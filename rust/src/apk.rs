use anyhow::{Context, Result, bail};
use flate2::read::DeflateDecoder;
use std::{
    fs,
    io::{Cursor, Read},
    path::Path,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApkMetadata {
    pub package_name: String,
    pub version_name: Option<String>,
    pub version_code: Option<String>,
}

const EOCD: u32 = 0x0605_4b50;
const CENTRAL_FILE: u32 = 0x0201_4b50;
const LOCAL_FILE: u32 = 0x0403_4b50;
const RES_XML_TYPE: u16 = 0x0003;
const RES_STRING_POOL_TYPE: u16 = 0x0001;
const RES_XML_START_ELEMENT_TYPE: u16 = 0x0102;
const TYPE_STRING: u8 = 0x03;
const TYPE_INT_DEC: u8 = 0x10;
const TYPE_INT_HEX: u8 = 0x11;
const MAX_MANIFEST_BYTES: usize = 16 * 1024 * 1024;

pub fn inspect_apk(path: &Path) -> Result<ApkMetadata> {
    if !path.is_file() {
        bail!("APK 不存在: {}", path.display());
    }
    let bytes = fs::read(path).with_context(|| format!("读取 APK 失败: {}", path.display()))?;
    let manifest = read_zip_entry(&bytes, "AndroidManifest.xml")
        .with_context(|| format!("APK 缺少 AndroidManifest.xml: {}", path.display()))?;
    parse_binary_manifest(&manifest)
}

#[derive(Debug, Clone, Copy)]
struct ZipEntry {
    flags: u16,
    method: u16,
    crc32: u32,
    compressed_size: usize,
    uncompressed_size: usize,
    local_offset: usize,
    central_offset: usize,
}

fn read_zip_entry(archive: &[u8], wanted: &str) -> Result<Vec<u8>> {
    let eocd = find_eocd(archive).context("APK ZIP 目录损坏")?;
    let disk = le_u16(archive, eocd.checked_add(4).context("ZIP EOCD 偏移溢出")?)?;
    let central_disk = le_u16(archive, eocd.checked_add(6).context("ZIP EOCD 偏移溢出")?)?;
    let entries_on_disk = le_u16(archive, eocd.checked_add(8).context("ZIP EOCD 偏移溢出")?)?;
    let entries_total = le_u16(archive, eocd.checked_add(10).context("ZIP EOCD 偏移溢出")?)?;
    if disk != 0 || central_disk != 0 || entries_on_disk != entries_total {
        bail!("APK 不支持跨磁盘 ZIP 目录");
    }
    let central_size_u32 = le_u32(archive, eocd.checked_add(12).context("ZIP EOCD 偏移溢出")?)?;
    let central_offset_u32 = le_u32(archive, eocd.checked_add(16).context("ZIP EOCD 偏移溢出")?)?;
    if central_size_u32 == u32::MAX || central_offset_u32 == u32::MAX || entries_total == u16::MAX {
        bail!("APK ZIP64 中央目录当前不受支持");
    }
    let central_size = central_size_u32 as usize;
    let central_offset = central_offset_u32 as usize;
    let central_end = central_offset
        .checked_add(central_size)
        .context("APK ZIP 中央目录长度溢出")?;
    if central_end != eocd || central_end > archive.len() {
        bail!("APK ZIP 中央目录位置或长度无效");
    }

    let mut cursor = central_offset;
    let mut wanted_entry = None;
    for _ in 0..entries_total {
        let fixed_end = cursor.checked_add(46).context("ZIP 条目偏移溢出")?;
        let fixed = archive
            .get(cursor..fixed_end)
            .context("APK ZIP 中央目录条目截断")?;
        if le_u32(fixed, 0)? != CENTRAL_FILE {
            bail!("APK ZIP 中央目录条目无效");
        }
        let flags = le_u16(fixed, 8)?;
        let method = le_u16(fixed, 10)?;
        let crc32 = le_u32(fixed, 16)?;
        let compressed_u32 = le_u32(fixed, 20)?;
        let uncompressed_u32 = le_u32(fixed, 24)?;
        let name_len = le_u16(fixed, 28)? as usize;
        let extra_len = le_u16(fixed, 30)? as usize;
        let comment_len = le_u16(fixed, 32)? as usize;
        let start_disk = le_u16(fixed, 34)?;
        let local_offset_u32 = le_u32(fixed, 42)?;
        if start_disk != 0 {
            bail!("APK ZIP 条目位于不支持的磁盘");
        }
        if compressed_u32 == u32::MAX
            || uncompressed_u32 == u32::MAX
            || local_offset_u32 == u32::MAX
        {
            bail!("APK ZIP64 条目当前不受支持");
        }
        let name_start = fixed_end;
        let name_end = name_start
            .checked_add(name_len)
            .context("APK ZIP 文件名长度溢出")?;
        let extra_end = name_end
            .checked_add(extra_len)
            .context("APK ZIP 扩展区长度溢出")?;
        let next = extra_end
            .checked_add(comment_len)
            .context("APK ZIP 注释长度溢出")?;
        if next > central_end {
            bail!("APK ZIP 中央目录条目越界");
        }
        let name_bytes = archive
            .get(name_start..name_end)
            .context("APK ZIP 文件名越界")?;
        let name = std::str::from_utf8(name_bytes)?;
        if name == wanted {
            if flags & 0x0041 != 0 {
                bail!("APK AndroidManifest.xml 使用了不支持的 ZIP 加密");
            }
            if wanted_entry.is_some() {
                bail!("APK ZIP 中存在重复的 {} 条目", wanted);
            }
            wanted_entry = Some((
                name_bytes.to_vec(),
                ZipEntry {
                    flags,
                    method,
                    crc32,
                    compressed_size: compressed_u32 as usize,
                    uncompressed_size: uncompressed_u32 as usize,
                    local_offset: local_offset_u32 as usize,
                    central_offset,
                },
            ));
        }
        cursor = next;
    }
    if cursor != central_end {
        bail!("APK ZIP 中央目录条目数量与目录长度不一致");
    }
    let (name, entry) = wanted_entry.with_context(|| format!("APK ZIP 中未找到: {}", wanted))?;
    read_local_entry(archive, &name, entry)
}

fn read_local_entry(archive: &[u8], wanted_name: &[u8], entry: ZipEntry) -> Result<Vec<u8>> {
    if entry.uncompressed_size > MAX_MANIFEST_BYTES {
        bail!("AndroidManifest.xml 超过允许大小限制");
    }
    let offset = entry.local_offset;
    let fixed_end = offset.checked_add(30).context("ZIP 本地头偏移溢出")?;
    let fixed = archive
        .get(offset..fixed_end)
        .context("APK ZIP 本地头截断")?;
    if le_u32(fixed, 0)? != LOCAL_FILE {
        bail!("APK ZIP 本地条目无效");
    }
    let local_flags = le_u16(fixed, 6)?;
    let local_method = le_u16(fixed, 8)?;
    if local_flags != entry.flags || local_method != entry.method {
        bail!("APK ZIP 本地头与中央目录标记不一致");
    }
    let local_crc = le_u32(fixed, 14)?;
    let local_compressed = le_u32(fixed, 18)?;
    let local_uncompressed = le_u32(fixed, 22)?;
    if entry.flags & 0x0008 == 0
        && (local_crc != entry.crc32
            || local_compressed as usize != entry.compressed_size
            || local_uncompressed as usize != entry.uncompressed_size)
    {
        bail!("APK ZIP 本地头长度/CRC 与中央目录不一致");
    }
    let name_len = le_u16(fixed, 26)? as usize;
    let extra_len = le_u16(fixed, 28)? as usize;
    let name_start = fixed_end;
    let name_end = name_start
        .checked_add(name_len)
        .context("APK ZIP 本地文件名长度溢出")?;
    let data_start = name_end
        .checked_add(extra_len)
        .context("APK ZIP 本地扩展区长度溢出")?;
    let local_name = archive
        .get(name_start..name_end)
        .context("APK ZIP 本地文件名越界")?;
    if local_name != wanted_name {
        bail!("APK ZIP 本地文件名与中央目录不一致");
    }
    let data_end = data_start
        .checked_add(entry.compressed_size)
        .context("APK ZIP 数据长度溢出")?;
    if data_end > entry.central_offset {
        bail!("APK ZIP 本地数据与中央目录重叠");
    }
    let compressed = archive
        .get(data_start..data_end)
        .context("APK ZIP 数据越界")?;
    let output = match entry.method {
        0 => compressed.to_vec(),
        8 => {
            let decoder = DeflateDecoder::new(Cursor::new(compressed));
            let mut output = Vec::with_capacity(entry.uncompressed_size.min(MAX_MANIFEST_BYTES));
            decoder
                .take((MAX_MANIFEST_BYTES as u64) + 1)
                .read_to_end(&mut output)
                .context("解压 AndroidManifest.xml 失败")?;
            output
        }
        other => bail!("APK 使用不支持的 ZIP 压缩方法: {}", other),
    };
    if output.len() != entry.uncompressed_size || output.len() > MAX_MANIFEST_BYTES {
        bail!("APK AndroidManifest.xml 解压长度与 ZIP 目录不一致");
    }
    if crc32(&output) != entry.crc32 {
        bail!("APK AndroidManifest.xml CRC 校验失败");
    }
    Ok(output)
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn find_eocd(bytes: &[u8]) -> Option<usize> {
    const EOCD_FIXED_SIZE: usize = 22;
    if bytes.len() < EOCD_FIXED_SIZE {
        return None;
    }
    let first = bytes
        .len()
        .saturating_sub(EOCD_FIXED_SIZE + u16::MAX as usize);
    (first..=bytes.len() - EOCD_FIXED_SIZE)
        .rev()
        .find(|&offset| {
            le_u32(bytes, offset).ok() == Some(EOCD)
                && le_u16(bytes, offset + 20).ok().and_then(|comment_size| {
                    offset.checked_add(EOCD_FIXED_SIZE + comment_size as usize)
                }) == Some(bytes.len())
        })
}

fn le_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let end = offset.checked_add(2).context("数据偏移溢出")?;
    Ok(u16::from_le_bytes(
        bytes.get(offset..end).context("数据越界")?.try_into()?,
    ))
}
fn le_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let end = offset.checked_add(4).context("数据偏移溢出")?;
    Ok(u32::from_le_bytes(
        bytes.get(offset..end).context("数据越界")?.try_into()?,
    ))
}

fn parse_binary_manifest(bytes: &[u8]) -> Result<ApkMetadata> {
    if le_u16(bytes, 0)? != RES_XML_TYPE {
        bail!("AndroidManifest.xml 不是 Android binary XML");
    }
    let root_header_size = le_u16(bytes, 2)? as usize;
    let root_chunk_size = le_u32(bytes, 4)? as usize;
    if root_header_size < 8 || root_header_size > root_chunk_size || root_chunk_size != bytes.len()
    {
        bail!("Android binary XML 头部长度无效");
    }

    let mut strings = Vec::new();
    let mut manifest = None;
    let mut offset = root_header_size;
    while offset < root_chunk_size {
        if root_chunk_size - offset < 8 {
            bail!("Android binary XML chunk 头部截断");
        }
        let chunk_type = le_u16(bytes, offset)?;
        let header_size = le_u16(bytes, offset + 2)? as usize;
        let chunk_size = le_u32(bytes, offset + 4)? as usize;
        let end = offset
            .checked_add(chunk_size)
            .context("Android binary XML chunk 长度溢出")?;
        if header_size < 8 || chunk_size < header_size || end > root_chunk_size {
            bail!("Android binary XML chunk 越界或长度无效");
        }
        if chunk_type == RES_STRING_POOL_TYPE {
            strings = parse_string_pool(&bytes[offset..end])?;
        } else if chunk_type == RES_XML_START_ELEMENT_TYPE
            && !strings.is_empty()
            && let Some(metadata) = parse_manifest_start(&bytes[offset..end], &strings)?
            && manifest.replace(metadata).is_some()
        {
            bail!("AndroidManifest.xml 含有重复 manifest 节点");
        }
        offset = end;
    }
    manifest.context("AndroidManifest.xml 未找到 manifest 节点")
}
fn parse_string_pool(chunk: &[u8]) -> Result<Vec<String>> {
    let header_size = le_u16(chunk, 2)? as usize;
    let string_count = le_u32(chunk, 8)? as usize;
    let style_count = le_u32(chunk, 12)? as usize;
    let flags = le_u32(chunk, 16)?;
    let strings_offset = le_u32(chunk, 20)? as usize;
    let string_offsets_size = string_count
        .checked_mul(4)
        .context("Android string count 溢出")?;
    let style_offsets_size = style_count
        .checked_mul(4)
        .context("Android style count 溢出")?;
    let offsets_end = 28usize
        .checked_add(string_offsets_size)
        .and_then(|end| end.checked_add(style_offsets_size))
        .context("Android string pool offsets 溢出")?;
    if header_size < 28
        || header_size > chunk.len()
        || offsets_end > chunk.len()
        || strings_offset < offsets_end
        || strings_offset > chunk.len()
    {
        bail!("Android string pool 越界或头部无效");
    }
    let utf8 = flags & 0x100 != 0;
    (0..string_count)
        .map(|index| {
            let offset = 28usize
                .checked_add(index.checked_mul(4).context("Android string index 溢出")?)
                .context("Android string index 溢出")?;
            let relative = le_u32(chunk, offset)? as usize;
            let start = strings_offset
                .checked_add(relative)
                .context("Android string 偏移溢出")?;
            if utf8 {
                decode_utf8_string(chunk, start)
            } else {
                decode_utf16_string(chunk, start)
            }
        })
        .collect()
}
fn parse_manifest_start(chunk: &[u8], strings: &[String]) -> Result<Option<ApkMetadata>> {
    let header_size = le_u16(chunk, 2)? as usize;
    if header_size < 16 || chunk.len() < header_size + 20 {
        return Ok(None);
    }
    let name_index = le_u32(chunk, 20)? as usize;
    if strings.get(name_index).map(String::as_str) != Some("manifest") {
        return Ok(None);
    }

    let attr_start = le_u16(chunk, 24)? as usize;
    let attr_size = le_u16(chunk, 26)? as usize;
    let attr_count = le_u16(chunk, 28)? as usize;
    if attr_count > 0 && attr_size < 20 {
        bail!("Android manifest 属性大小无效");
    }
    let attrs = 16usize
        .checked_add(attr_start)
        .context("Android 属性偏移溢出")?;
    let attrs_end = attr_count
        .checked_mul(attr_size)
        .and_then(|size| attrs.checked_add(size))
        .context("Android manifest 属性长度溢出")?;
    if attrs < header_size || attrs_end > chunk.len() {
        bail!("Android manifest 属性越界");
    }

    let mut package_name = None;
    let mut version_name = None;
    let mut version_code = None;
    for index in 0..attr_count {
        let start = attrs
            .checked_add(
                index
                    .checked_mul(attr_size)
                    .context("Android 属性索引溢出")?,
            )
            .context("Android 属性偏移溢出")?;
        let name_index = le_u32(chunk, start + 4)? as usize;
        let name = strings
            .get(name_index)
            .context("Android manifest 属性名称索引越界")?
            .as_str();
        let value_type = chunk
            .get(start + 15)
            .copied()
            .context("Android 属性类型越界")?;
        let value_data = le_u32(chunk, start + 16)?;

        let value = match (name, value_type) {
            ("package" | "versionName", TYPE_STRING) => Some(
                strings
                    .get(value_data as usize)
                    .cloned()
                    .context("Android manifest 字符串属性值索引越界")?,
            ),
            ("versionCode", TYPE_INT_DEC) => Some(value_data.to_string()),
            ("versionCode", TYPE_INT_HEX) => Some(format!("0x{:x}", value_data)),
            ("package" | "versionName" | "versionCode", _) => {
                bail!("Android manifest 属性类型无效: {name}");
            }
            _ => None,
        };
        let Some(value) = value else { continue };
        match name {
            "package" => {
                if package_name.is_some() {
                    bail!("Android manifest 含有重复 package 属性");
                }
                package_name = Some(value);
            }
            "versionName" => {
                if version_name.is_some() {
                    bail!("Android manifest 含有重复 versionName 属性");
                }
                version_name = Some(value);
            }
            "versionCode" => {
                if version_code.is_some() {
                    bail!("Android manifest 含有重复 versionCode 属性");
                }
                version_code = Some(value);
            }
            _ => {}
        }
    }
    let package_name = package_name.context("AndroidManifest.xml 缺少 package")?;
    if package_name.trim().is_empty() {
        bail!("AndroidManifest.xml package 为空");
    }
    Ok(Some(ApkMetadata {
        package_name,
        version_name,
        version_code,
    }))
}
fn decode_utf8_string(bytes: &[u8], start: usize) -> Result<String> {
    let (length, cursor) = decode_length8(bytes, start)?;
    let (_, cursor) = decode_length8(bytes, cursor)?;
    let end = cursor.checked_add(length).context("UTF-8 string 越界")?;
    Ok(String::from_utf8(
        bytes
            .get(cursor..end)
            .context("UTF-8 string 越界")?
            .to_vec(),
    )?)
}
fn decode_utf16_string(bytes: &[u8], start: usize) -> Result<String> {
    let (length, cursor) = decode_length16(bytes, start)?;
    let byte_length = length.checked_mul(2).context("UTF-16 string 长度溢出")?;
    let end = cursor
        .checked_add(byte_length)
        .context("UTF-16 string 越界")?;
    let encoded = bytes.get(cursor..end).context("UTF-16 string 越界")?;
    let values: Vec<u16> = encoded
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    Ok(String::from_utf16(&values)?)
}
fn decode_length8(bytes: &[u8], start: usize) -> Result<(usize, usize)> {
    let first = *bytes.get(start).context("UTF-8 长度越界")?;
    if first & 0x80 == 0 {
        Ok((first as usize, start + 1))
    } else {
        Ok((
            ((first as usize & 0x7f) << 8)
                | *bytes.get(start + 1).context("UTF-8 长度越界")? as usize,
            start + 2,
        ))
    }
}
fn decode_length16(bytes: &[u8], start: usize) -> Result<(usize, usize)> {
    let first = le_u16(bytes, start)?;
    if first & 0x8000 == 0 {
        Ok((first as usize, start + 2))
    } else {
        Ok((
            ((first as usize & 0x7fff) << 16) | le_u16(bytes, start + 2)? as usize,
            start + 4,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL_MANIFEST: &[u8] = include_bytes!("testdata/uiautomator2_AndroidManifest.xml");

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn stored_zip(entries: &[(&str, &[u8])], comment: &[u8]) -> Vec<u8> {
        let mut output = Vec::new();
        let mut central = Vec::new();
        for (name, data) in entries {
            let local_offset = output.len() as u32;
            let crc = crc32(data);
            let name = name.as_bytes();
            output.extend_from_slice(&LOCAL_FILE.to_le_bytes());
            output.extend_from_slice(&20u16.to_le_bytes());
            output.extend_from_slice(&0u16.to_le_bytes());
            output.extend_from_slice(&0u16.to_le_bytes());
            output.extend_from_slice(&0u16.to_le_bytes());
            output.extend_from_slice(&0u16.to_le_bytes());
            output.extend_from_slice(&crc.to_le_bytes());
            output.extend_from_slice(&(data.len() as u32).to_le_bytes());
            output.extend_from_slice(&(data.len() as u32).to_le_bytes());
            output.extend_from_slice(&(name.len() as u16).to_le_bytes());
            output.extend_from_slice(&0u16.to_le_bytes());
            output.extend_from_slice(name);
            output.extend_from_slice(data);

            central.extend_from_slice(&CENTRAL_FILE.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&local_offset.to_le_bytes());
            central.extend_from_slice(name);
        }
        let central_offset = output.len() as u32;
        output.extend_from_slice(&central);
        output.extend_from_slice(&EOCD.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        output.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        output.extend_from_slice(&(central.len() as u32).to_le_bytes());
        output.extend_from_slice(&central_offset.to_le_bytes());
        output.extend_from_slice(&(comment.len() as u16).to_le_bytes());
        output.extend_from_slice(comment);
        output
    }

    fn deflated_zip(entry_name: &str, data: &[u8]) -> Vec<u8> {
        use flate2::{Compression, write::DeflateEncoder};
        use std::io::Write as _;

        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        let compressed = encoder.finish().unwrap();
        let name = entry_name.as_bytes();
        let crc = crc32(data);
        let mut archive = Vec::new();
        archive.extend_from_slice(&LOCAL_FILE.to_le_bytes());
        archive.extend_from_slice(&20u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&8u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&crc.to_le_bytes());
        archive.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        archive.extend_from_slice(&(data.len() as u32).to_le_bytes());
        archive.extend_from_slice(&(name.len() as u16).to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(name);
        archive.extend_from_slice(&compressed);

        let central_offset = archive.len() as u32;
        archive.extend_from_slice(&CENTRAL_FILE.to_le_bytes());
        archive.extend_from_slice(&20u16.to_le_bytes());
        archive.extend_from_slice(&20u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&8u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&crc.to_le_bytes());
        archive.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        archive.extend_from_slice(&(data.len() as u32).to_le_bytes());
        archive.extend_from_slice(&(name.len() as u16).to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u32.to_le_bytes());
        archive.extend_from_slice(&0u32.to_le_bytes());
        archive.extend_from_slice(name);
        let central_size = archive.len() as u32 - central_offset;
        archive.extend_from_slice(&EOCD.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&1u16.to_le_bytes());
        archive.extend_from_slice(&1u16.to_le_bytes());
        archive.extend_from_slice(&central_size.to_le_bytes());
        archive.extend_from_slice(&central_offset.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive
    }

    fn stored_zip_with_data_descriptor(entry_name: &str, data: &[u8]) -> Vec<u8> {
        let mut archive = stored_zip(&[(entry_name, data)], b"");
        let eocd = find_eocd(&archive).unwrap();
        let central_offset = le_u32(&archive, eocd + 16).unwrap() as usize;
        let local_offset = le_u32(&archive, central_offset + 42).unwrap() as usize;
        let name_len = le_u16(&archive, local_offset + 26).unwrap() as usize;
        let extra_len = le_u16(&archive, local_offset + 28).unwrap() as usize;
        let data_end = local_offset + 30 + name_len + extra_len + data.len();
        let crc = crc32(data);
        let mut descriptor = Vec::new();
        descriptor.extend_from_slice(&0x0807_4b50u32.to_le_bytes());
        descriptor.extend_from_slice(&crc.to_le_bytes());
        descriptor.extend_from_slice(&(data.len() as u32).to_le_bytes());
        descriptor.extend_from_slice(&(data.len() as u32).to_le_bytes());
        archive.splice(data_end..data_end, descriptor);
        let eocd = find_eocd(&archive).unwrap();
        let shifted_central_offset = le_u32(&archive, eocd + 16).unwrap() + 16;
        put_u32(&mut archive, eocd + 16, shifted_central_offset);
        put_u16(&mut archive, local_offset + 6, 0x0008);
        put_u16(&mut archive, shifted_central_offset as usize + 8, 0x0008);
        put_u32(&mut archive, local_offset + 14, 0);
        put_u32(&mut archive, local_offset + 18, 0);
        put_u32(&mut archive, local_offset + 22, 0);
        archive
    }
    fn apk_metadata() -> ApkMetadata {
        ApkMetadata {
            package_name: "com.github.uiautomator".into(),
            version_name: Some("2.4.0".into()),
            version_code: Some("2004001".into()),
        }
    }

    #[test]
    fn parses_real_world_binary_manifest_fixture() {
        let metadata = parse_binary_manifest(REAL_MANIFEST).unwrap();
        assert_eq!(metadata, apk_metadata());
    }

    #[test]
    fn extracts_real_world_manifest_through_stored_zip() {
        let archive = stored_zip(
            &[
                ("classes.dex", b"dex\n035\0"),
                ("AndroidManifest.xml", REAL_MANIFEST),
            ],
            b"APK fixture",
        );
        let manifest = read_zip_entry(&archive, "AndroidManifest.xml").unwrap();
        assert_eq!(parse_binary_manifest(&manifest).unwrap(), apk_metadata());
    }

    #[test]
    fn extracts_real_world_manifest_through_deflated_zip() {
        let archive = deflated_zip("AndroidManifest.xml", REAL_MANIFEST);
        let manifest = read_zip_entry(&archive, "AndroidManifest.xml").unwrap();
        assert_eq!(parse_binary_manifest(&manifest).unwrap(), apk_metadata());
    }

    #[test]
    fn finds_eocd_even_when_archive_comment_contains_signature_bytes() {
        let archive = stored_zip(
            &[("AndroidManifest.xml", REAL_MANIFEST)],
            b"fake PK\x05\x06 in comment",
        );
        assert_eq!(
            parse_binary_manifest(&read_zip_entry(&archive, "AndroidManifest.xml").unwrap())
                .unwrap(),
            apk_metadata()
        );
    }

    #[test]
    fn rejects_local_name_mismatch_and_central_or_payload_crc_mismatch() {
        let mut archive = stored_zip(&[("AndroidManifest.xml", REAL_MANIFEST)], b"");
        let central = archive
            .windows(4)
            .rposition(|window| window == CENTRAL_FILE.to_le_bytes())
            .unwrap();
        let local_offset = le_u32(&archive, central + 42).unwrap() as usize;
        archive[local_offset + 30] ^= 1;
        assert!(read_zip_entry(&archive, "AndroidManifest.xml").is_err());

        let mut archive = stored_zip(&[("AndroidManifest.xml", REAL_MANIFEST)], b"");
        let central = archive
            .windows(4)
            .rposition(|window| window == CENTRAL_FILE.to_le_bytes())
            .unwrap();
        archive[central + 16] ^= 0xff;
        assert!(read_zip_entry(&archive, "AndroidManifest.xml").is_err());

        let mut archive = stored_zip(&[("AndroidManifest.xml", REAL_MANIFEST)], b"");
        let data_offset = 30 + "AndroidManifest.xml".len();
        archive[data_offset + 20] ^= 1;
        assert!(read_zip_entry(&archive, "AndroidManifest.xml").is_err());
    }

    #[test]
    fn rejects_malformed_xml_chunk_lengths_and_string_pool_offsets() {
        let mut bad_root_size = REAL_MANIFEST.to_vec();
        put_u32(&mut bad_root_size, 4, u32::MAX);
        assert!(parse_binary_manifest(&bad_root_size).is_err());

        let mut bad_chunk_size = REAL_MANIFEST.to_vec();
        put_u32(&mut bad_chunk_size, 12, u32::MAX);
        assert!(parse_binary_manifest(&bad_chunk_size).is_err());

        let mut bad_string_offset = REAL_MANIFEST.to_vec();
        put_u32(&mut bad_string_offset, 36, u32::MAX);
        assert!(parse_binary_manifest(&bad_string_offset).is_err());
    }

    #[test]
    fn rejects_zero_length_chunks_and_duplicate_manifest_nodes() {
        let mut zero_chunk = REAL_MANIFEST.to_vec();
        put_u16(&mut zero_chunk, 10, 0);
        put_u32(&mut zero_chunk, 12, 0);
        assert!(parse_binary_manifest(&zero_chunk).is_err());

        let mut duplicate = REAL_MANIFEST.to_vec();
        let pool_start = le_u16(&duplicate, 2).unwrap() as usize;
        let pool_size = le_u32(&duplicate, pool_start + 4).unwrap() as usize;
        let strings = parse_string_pool(&duplicate[pool_start..pool_start + pool_size]).unwrap();
        let mut cursor = pool_start + pool_size;
        let manifest_element = loop {
            if cursor + 8 > duplicate.len() {
                panic!("manifest start-element fixture missing");
            }
            let chunk_size = le_u32(&duplicate, cursor + 4).unwrap() as usize;
            let end = cursor + chunk_size;
            if le_u16(&duplicate, cursor).unwrap() == RES_XML_START_ELEMENT_TYPE
                && parse_manifest_start(&duplicate[cursor..end], &strings)
                    .unwrap()
                    .is_some()
            {
                break duplicate[cursor..end].to_vec();
            }
            cursor = end;
        };
        duplicate.extend_from_slice(&manifest_element);
        let duplicate_size = duplicate.len() as u32;
        put_u32(&mut duplicate, 4, duplicate_size);
        assert!(parse_binary_manifest(&duplicate).is_err());
    }

    #[test]
    fn validates_the_entire_central_directory_and_rejects_truncated_zip_variants() {
        let mut trailing_bad_entry = stored_zip(
            &[
                ("AndroidManifest.xml", REAL_MANIFEST),
                ("classes.dex", b"dex"),
            ],
            b"",
        );
        let eocd = find_eocd(&trailing_bad_entry).unwrap();
        let central = le_u32(&trailing_bad_entry, eocd + 16).unwrap() as usize;
        let second = central + 46 + "AndroidManifest.xml".len();
        trailing_bad_entry[second] ^= 0xff;
        assert!(read_zip_entry(&trailing_bad_entry, "AndroidManifest.xml").is_err());

        let archive = stored_zip(&[("AndroidManifest.xml", REAL_MANIFEST)], b"");
        assert!(read_zip_entry(&archive[..archive.len() - 1], "AndroidManifest.xml").is_err());

        let mut truncated_directory = archive.clone();
        let eocd = find_eocd(&truncated_directory).unwrap();
        let size = le_u32(&truncated_directory, eocd + 12).unwrap();
        put_u32(&mut truncated_directory, eocd + 12, size - 1);
        assert!(read_zip_entry(&truncated_directory, "AndroidManifest.xml").is_err());

        let mut zip64 = archive.clone();
        let eocd = find_eocd(&zip64).unwrap();
        put_u16(&mut zip64, eocd + 8, u16::MAX);
        put_u16(&mut zip64, eocd + 10, u16::MAX);
        assert!(read_zip_entry(&zip64, "AndroidManifest.xml").is_err());

        let mut multidisk = archive.clone();
        let eocd = find_eocd(&multidisk).unwrap();
        put_u16(&mut multidisk, eocd + 4, 1);
        assert!(read_zip_entry(&multidisk, "AndroidManifest.xml").is_err());

        let mut strong_encryption = archive.clone();
        let central = strong_encryption
            .windows(4)
            .rposition(|window| window == CENTRAL_FILE.to_le_bytes())
            .unwrap();
        put_u16(&mut strong_encryption, central + 8, 0x0040);
        assert!(read_zip_entry(&strong_encryption, "AndroidManifest.xml").is_err());

        let mut truncated_local_header = archive;
        let central = truncated_local_header
            .windows(4)
            .rposition(|window| window == CENTRAL_FILE.to_le_bytes())
            .unwrap();
        put_u32(&mut truncated_local_header, central + 42, u32::MAX - 1);
        assert!(read_zip_entry(&truncated_local_header, "AndroidManifest.xml").is_err());
    }
    #[test]
    fn accepts_consistent_data_descriptor_using_central_directory_metadata() {
        let archive = stored_zip_with_data_descriptor("AndroidManifest.xml", REAL_MANIFEST);
        let manifest = read_zip_entry(&archive, "AndroidManifest.xml").unwrap();
        assert_eq!(parse_binary_manifest(&manifest).unwrap(), apk_metadata());
    }
    #[test]
    fn rejects_duplicate_manifest_zip_entries_even_when_first_is_valid() {
        let archive = stored_zip(
            &[
                ("AndroidManifest.xml", REAL_MANIFEST),
                ("AndroidManifest.xml", REAL_MANIFEST),
            ],
            b"",
        );
        assert!(read_zip_entry(&archive, "AndroidManifest.xml").is_err());
    }
    #[test]
    fn rejects_manifest_zip_header_mismatches_and_oversized_entry() {
        for (field_offset, mask) in [(6usize, 1u8), (8, 1), (14, 1), (18, 1), (22, 1)] {
            let mut archive = stored_zip(&[("AndroidManifest.xml", REAL_MANIFEST)], b"");
            let central = archive
                .windows(4)
                .rposition(|window| window == CENTRAL_FILE.to_le_bytes())
                .unwrap();
            let local = le_u32(&archive, central + 42).unwrap() as usize;
            archive[local + field_offset] ^= mask;
            assert!(read_zip_entry(&archive, "AndroidManifest.xml").is_err());
        }

        let mut oversized = stored_zip(&[("AndroidManifest.xml", REAL_MANIFEST)], b"");
        let central = oversized
            .windows(4)
            .rposition(|window| window == CENTRAL_FILE.to_le_bytes())
            .unwrap();
        put_u32(
            &mut oversized,
            central + 24,
            (MAX_MANIFEST_BYTES + 1) as u32,
        );
        assert!(read_zip_entry(&oversized, "AndroidManifest.xml").is_err());
    }
    #[test]
    fn rejects_unknown_type_for_known_version_attribute() {
        let mut malformed = REAL_MANIFEST.to_vec();
        let pool_start = le_u16(&malformed, 2).unwrap() as usize;
        let pool_size = le_u32(&malformed, pool_start + 4).unwrap() as usize;
        let strings = parse_string_pool(&malformed[pool_start..pool_start + pool_size]).unwrap();
        let mut cursor = pool_start + pool_size;
        let mut changed = false;
        while cursor + 8 <= malformed.len() {
            let chunk_size = le_u32(&malformed, cursor + 4).unwrap() as usize;
            let chunk_end = cursor + chunk_size;
            if le_u16(&malformed, cursor).unwrap() == RES_XML_START_ELEMENT_TYPE
                && parse_manifest_start(&malformed[cursor..chunk_end], &strings)
                    .unwrap()
                    .is_some()
            {
                let attrs = cursor + 16 + le_u16(&malformed, cursor + 24).unwrap() as usize;
                let attr_size = le_u16(&malformed, cursor + 26).unwrap() as usize;
                let attr_count = le_u16(&malformed, cursor + 28).unwrap() as usize;
                for index in 0..attr_count {
                    let attr = attrs + index * attr_size;
                    let name_index = le_u32(&malformed, attr + 4).unwrap() as usize;
                    if strings.get(name_index).map(String::as_str) == Some("versionName") {
                        malformed[attr + 15] = 0x7f;
                        changed = true;
                        break;
                    }
                }
                break;
            }
            cursor = chunk_end;
        }
        assert!(changed, "versionName attribute fixture not found");
        assert!(parse_binary_manifest(&malformed).is_err());
    }
    #[test]
    fn rejects_invalid_manifest_attributes_instead_of_empty_values() {
        let mut missing_package = REAL_MANIFEST.to_vec();
        let package_utf16: Vec<u8> = "package"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let package_index = missing_package
            .windows(package_utf16.len())
            .position(|window| window == package_utf16.as_slice())
            .unwrap();
        // Corrupt the string-pool name; the parser must not accept an empty package.
        missing_package[package_index] = b'x';
        assert!(parse_binary_manifest(&missing_package).is_err());

        let mut bad_utf16_length = [0u8; 2];
        put_u16(&mut bad_utf16_length, 0, u16::MAX);
        assert!(decode_utf16_string(&bad_utf16_length, 0).is_err());
    }

    #[test]
    fn rejects_unbounded_utf16_length_before_allocating() {
        // 0xFFFF, 0xFFFF encodes a 2,147,483,647-code-unit string but provides no payload.
        let claimed_huge_string = [0xffu8; 4];
        assert!(decode_utf16_string(&claimed_huge_string, 0).is_err());
    }
    #[test]
    fn rejects_non_binary_manifest() {
        assert!(parse_binary_manifest(b"<manifest />").is_err());
    }

    #[test]
    fn decodes_android_utf8_pool_length() {
        let bytes = [3u8, 3, b'a', b'p', b'k'];
        assert_eq!(decode_utf8_string(&bytes, 0).unwrap(), "apk");
    }
}
