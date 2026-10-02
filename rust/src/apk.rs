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

pub fn inspect_apk(path: &Path) -> Result<ApkMetadata> {
    if !path.is_file() {
        bail!("APK 不存在: {}", path.display());
    }
    let bytes = fs::read(path).with_context(|| format!("读取 APK 失败: {}", path.display()))?;
    let manifest = read_zip_entry(&bytes, "AndroidManifest.xml")
        .with_context(|| format!("APK 缺少 AndroidManifest.xml: {}", path.display()))?;
    parse_binary_manifest(&manifest)
}

fn read_zip_entry(archive: &[u8], wanted: &str) -> Result<Vec<u8>> {
    let eocd = find_signature_from_end(archive, EOCD).context("APK ZIP 目录损坏")?;
    let entries = le_u16(archive, eocd + 10)? as usize;
    let central_size = le_u32(archive, eocd + 12)? as usize;
    let central_offset = le_u32(archive, eocd + 16)? as usize;
    if central_offset
        .checked_add(central_size)
        .is_none_or(|end| end > archive.len())
    {
        bail!("APK ZIP 中央目录越界");
    }
    let mut cursor = central_offset;
    for _ in 0..entries {
        if le_u32(archive, cursor)? != CENTRAL_FILE {
            bail!("APK ZIP 中央目录条目无效");
        }
        let method = le_u16(archive, cursor + 10)?;
        let compressed_size = le_u32(archive, cursor + 20)? as usize;
        let name_len = le_u16(archive, cursor + 28)? as usize;
        let extra_len = le_u16(archive, cursor + 30)? as usize;
        let comment_len = le_u16(archive, cursor + 32)? as usize;
        let local_offset = le_u32(archive, cursor + 42)? as usize;
        let name_start = cursor + 46;
        let name_end = name_start
            .checked_add(name_len)
            .context("APK ZIP 文件名越界")?;
        let name = std::str::from_utf8(
            archive
                .get(name_start..name_end)
                .context("APK ZIP 文件名越界")?,
        )?;
        let next = name_end
            .checked_add(extra_len)
            .and_then(|value| value.checked_add(comment_len))
            .context("APK ZIP 条目越界")?;
        if name == wanted {
            return read_local_entry(archive, local_offset, method, compressed_size);
        }
        cursor = next;
    }
    bail!("APK ZIP 中未找到: {}", wanted)
}

fn read_local_entry(
    archive: &[u8],
    offset: usize,
    method: u16,
    compressed_size: usize,
) -> Result<Vec<u8>> {
    if le_u32(archive, offset)? != LOCAL_FILE {
        bail!("APK ZIP 本地条目无效");
    }
    let name_len = le_u16(archive, offset + 26)? as usize;
    let extra_len = le_u16(archive, offset + 28)? as usize;
    let data_start = offset
        .checked_add(30)
        .and_then(|value| value.checked_add(name_len))
        .and_then(|value| value.checked_add(extra_len))
        .context("APK ZIP 数据偏移越界")?;
    let data_end = data_start
        .checked_add(compressed_size)
        .context("APK ZIP 数据长度溢出")?;
    let compressed = archive
        .get(data_start..data_end)
        .context("APK ZIP 数据越界")?;
    match method {
        0 => Ok(compressed.to_vec()),
        8 => {
            let mut decoder = DeflateDecoder::new(Cursor::new(compressed));
            let mut output = Vec::new();
            decoder
                .read_to_end(&mut output)
                .context("解压 AndroidManifest.xml 失败")?;
            Ok(output)
        }
        other => bail!("APK 使用不支持的 ZIP 压缩方法: {}", other),
    }
}

fn parse_binary_manifest(bytes: &[u8]) -> Result<ApkMetadata> {
    if le_u16(bytes, 0)? != RES_XML_TYPE {
        bail!("AndroidManifest.xml 不是 Android binary XML");
    }
    let mut strings = Vec::new();
    let mut offset = le_u16(bytes, 2)? as usize;
    if offset < 8 || offset > bytes.len() {
        bail!("Android binary XML 头部无效");
    }
    while offset + 8 <= bytes.len() {
        let chunk_type = le_u16(bytes, offset)?;
        let header_size = le_u16(bytes, offset + 2)? as usize;
        let chunk_size = le_u32(bytes, offset + 4)? as usize;
        if chunk_size < header_size
            || offset
                .checked_add(chunk_size)
                .is_none_or(|end| end > bytes.len())
        {
            bail!("Android binary XML chunk 越界");
        }
        if chunk_type == RES_STRING_POOL_TYPE {
            strings = parse_string_pool(&bytes[offset..offset + chunk_size])?;
        } else if chunk_type == RES_XML_START_ELEMENT_TYPE
            && !strings.is_empty()
            && let Some(metadata) =
                parse_manifest_start(&bytes[offset..offset + chunk_size], &strings)?
        {
            return Ok(metadata);
        }
        offset += chunk_size;
    }
    bail!("AndroidManifest.xml 未找到 manifest 节点")
}

fn parse_string_pool(chunk: &[u8]) -> Result<Vec<String>> {
    let string_count = le_u32(chunk, 8)? as usize;
    let flags = le_u32(chunk, 16)?;
    let strings_offset = le_u32(chunk, 20)? as usize;
    if 28usize
        .checked_add(string_count * 4)
        .is_none_or(|end| end > chunk.len())
        || strings_offset > chunk.len()
    {
        bail!("Android string pool 越界");
    }
    let utf8 = flags & 0x100 != 0;
    (0..string_count)
        .map(|index| {
            let relative = le_u32(chunk, 28 + index * 4)? as usize;
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
    let attrs = 16usize
        .checked_add(attr_start)
        .context("Android 属性偏移溢出")?;
    let mut package_name = None;
    let mut version_name = None;
    let mut version_code = None;
    for index in 0..attr_count {
        let start = attrs + index * attr_size;
        if attr_size < 20 || start + 20 > chunk.len() {
            bail!("Android manifest 属性越界");
        }
        let name = strings
            .get(le_u32(chunk, start + 4)? as usize)
            .map(String::as_str)
            .unwrap_or("");
        let raw = le_u32(chunk, start + 8)? as usize;
        let value_type = chunk
            .get(start + 15)
            .copied()
            .context("Android 属性类型越界")?;
        let value_data = le_u32(chunk, start + 16)?;
        let value = match value_type {
            TYPE_STRING => strings.get(raw).cloned().unwrap_or_default(),
            TYPE_INT_DEC => value_data.to_string(),
            TYPE_INT_HEX => format!("0x{:x}", value_data),
            _ => continue,
        };
        match name {
            "package" => package_name = Some(value),
            "versionName" => version_name = Some(value),
            "versionCode" => version_code = Some(value),
            _ => {}
        }
    }
    let package_name = package_name.context("AndroidManifest.xml 缺少 package")?;
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
    let (length, mut cursor) = decode_length16(bytes, start)?;
    let end = cursor
        .checked_add(length * 2)
        .context("UTF-16 string 越界")?;
    let mut values = Vec::with_capacity(length);
    while cursor < end {
        values.push(le_u16(bytes, cursor)?);
        cursor += 2;
    }
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
fn find_signature_from_end(bytes: &[u8], signature: u32) -> Option<usize> {
    bytes.windows(4).rposition(|window| {
        let array: [u8; 4] = window.try_into().expect("window size is four");
        u32::from_le_bytes(array) == signature
    })
}
fn le_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset + 2)
            .context("数据越界")?
            .try_into()?,
    ))
}
fn le_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .context("数据越界")?
            .try_into()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_android_utf8_pool_length() {
        let bytes = [3u8, 3, b'a', b'p', b'k'];
        assert_eq!(decode_utf8_string(&bytes, 0).unwrap(), "apk");
    }
    #[test]
    fn rejects_non_binary_manifest() {
        assert!(parse_binary_manifest(b"<manifest />").is_err());
    }
}
