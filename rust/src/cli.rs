//! Command-line parser definitions.
//!
//! This module is the only place that depends on `clap`. Business code, the GUI,
//! and the run state machine consume the typed requests in [`crate::commands`].

use crate::VERSION;
use crate::commands::{ConfigRequest, InstanceTargetArgs, RequestedMode, RunRequest, TaskRequest};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name="zhfd-checkin",
    version=VERSION,
    about="智汇福大 LDPlayer 自动签到控制器",
    after_help = "首次使用（只读，不点击）：\n  zhfd-checkin.exe about\n  zhfd-checkin.exe config path\n  zhfd-checkin.exe diagnose --instance-index 0\n  zhfd-checkin.exe run --dry-run --instance-index 0\n\n无子命令将启动 GUI；脚本必须显式传子命令。\nlive 会真实点击签到按钮，必须同时传 --live --confirm；dry-run 永不点击签到按钮。\nlogin 是会话建立动作：无 --confirm-login 时只探测，不会点击签到控件。"
)]
pub(crate) struct Cli {
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help = "显式指定 config.toml；其父目录即为 data root"
    )]
    pub(crate) config: Option<PathBuf>,
    #[command(subcommand)]
    pub(crate) command: Option<CommandKind>,
}

impl Cli {
    /// Parse `std::env::args_os`; this is the only clap entry point the rest of
    /// the crate uses.
    pub(crate) fn parse_args() -> Self {
        <Self as Parser>::parse()
    }
}

#[derive(Subcommand, Debug)]
pub(crate) enum CommandKind {
    /// 只读环境与设备诊断（不点击、不修改配置）
    #[command(visible_alias = "doctor")]
    Diagnose {
        #[command(flatten)]
        target: CliInstanceTargetArgs,
    },
    /// 生成默认脱敏的诊断报告（reports/diagnostic-*.json）
    Report,
    /// Profile 查询、检测与同步
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// 运行签到状态机（默认跟随 runtime.mode；live 必须 --confirm）
    Run(CliRunArgs),
    /// 只登录：在登录页执行一次一键登录，不进入签到、不要求 Profile 标定
    Login(LoginArgs),
    /// APK 安装
    App {
        #[command(subcommand)]
        command: AppCommand,
    },
    /// 配置查询与打开（show 默认脱敏）
    Config {
        #[command(subcommand)]
        command: CliConfigCommand,
    },
    /// 版本与构建来源
    About,
    /// 打开 GUI（无子命令时也会进入这里）
    Gui,
    /// 实例列表
    Instance {
        #[command(subcommand)]
        command: InstanceCommand,
    },
    /// Windows 计划任务
    Task {
        #[command(subcommand)]
        command: CliTaskCommand,
    },
    /// 原始 UI hierarchy（可能含账号/姓名等 PII）
    Ui {
        #[command(subcommand)]
        command: UiCommand,
    },
    /// 离线图片分类（无需配置）
    #[command(visible_alias = "analyze-image")]
    Vision {
        #[arg(long, value_name = "FILE", help = "要分类的本地截图路径")]
        image: PathBuf,
    },
}
#[derive(Subcommand, Debug)]
pub(crate) enum ProfileCommand {
    /// 列出内置 Profile
    List,
    /// 检测当前实例的分辨率/DPI 并匹配 Profile
    Detect {
        #[command(flatten)]
        target: CliInstanceTargetArgs,
    },
    /// 修改模拟器分辨率/DPI 并重启实例（危险，必须 --confirm）
    Sync {
        #[arg(
            long,
            value_name = "ID",
            help = "要同步的 Profile ID，例如 portrait_900x1600_d320"
        )]
        id: String,
        #[arg(long, required = true, help = "显式确认修改分辨率/DPI 并重启实例")]
        confirm: bool,
        #[command(flatten)]
        target: CliInstanceTargetArgs,
    },
}
#[derive(Subcommand, Debug)]
pub(crate) enum AppCommand {
    /// 安装 APK 到目标实例（可能自动启动模拟器并写入配置）
    Install {
        #[arg(long, value_name = "APK", help = "APK 文件路径")]
        apk: PathBuf,
        #[arg(
            long,
            required = true,
            help = "显式确认安装/替换 APK（可能重启模拟器并写入配置）"
        )]
        confirm: bool,
        #[command(flatten)]
        target: CliInstanceTargetArgs,
    },
}
#[derive(Subcommand, Debug)]
pub(crate) enum CliConfigCommand {
    /// 输出解析后的配置路径、数据根与来源
    Path,
    /// 输出配置（账号与通知地址默认脱敏）
    Show,
    /// 用系统编辑器打开明文配置文件
    Open,
}

impl From<CliConfigCommand> for ConfigRequest {
    fn from(command: CliConfigCommand) -> Self {
        match command {
            CliConfigCommand::Path => Self::Path,
            CliConfigCommand::Show => Self::Show,
            CliConfigCommand::Open => Self::Open,
        }
    }
}
#[derive(Subcommand, Debug)]
pub(crate) enum InstanceCommand {
    /// 列出 LDPlayer 与配置中的实例
    List,
}

#[derive(Args, Debug, Clone, Default)]
pub(crate) struct CliInstanceTargetArgs {
    #[arg(long, help = "选择 LDPlayer 实例 index；默认使用 config.toml")]
    pub(crate) instance_index: Option<u32>,
    #[arg(
        long,
        help = "显式指定 ADB serial；与 --instance-index 同时使用时必须指向同一实例"
    )]
    pub(crate) serial: Option<String>,
}

impl From<CliInstanceTargetArgs> for InstanceTargetArgs {
    fn from(value: CliInstanceTargetArgs) -> Self {
        Self {
            instance_index: value.instance_index,
            serial: value.serial,
        }
    }
}

impl From<&CliInstanceTargetArgs> for InstanceTargetArgs {
    fn from(value: &CliInstanceTargetArgs) -> Self {
        Self {
            instance_index: value.instance_index,
            serial: value.serial.clone(),
        }
    }
}

#[derive(Subcommand, Debug)]
pub(crate) enum CliTaskCommand {
    /// 创建每日计划任务（默认 dry-run；live 需 --confirm）
    Create {
        #[arg(
            long,
            default_value = "ZHFD-AutoCheckin",
            help = "Windows 计划任务名称"
        )]
        name: String,
        #[arg(
            long,
            conflicts_with = "live",
            help = "创建 dry-run 计划任务（默认；不点击签到）"
        )]
        dry_run: bool,
        #[arg(
            long,
            requires = "confirm",
            help = "让计划任务执行真实签到（高风险；必须同时传 --confirm）"
        )]
        live: bool,
        #[arg(long, requires = "live", help = "显式授权 live 计划任务")]
        confirm: bool,
        #[arg(
            long,
            conflicts_with_all = ["instance_index", "serial"],
            help = "依次运行配置中 enabled 的所有实例（不能与实例选择器组合）"
        )]
        all_instances: bool,
        #[command(flatten)]
        target: CliInstanceTargetArgs,
    },
    /// 删除计划任务
    Delete {
        #[arg(
            long,
            default_value = "ZHFD-AutoCheckin",
            help = "Windows 计划任务名称"
        )]
        name: String,
        #[arg(long, required = true, help = "显式确认删除计划任务（不可恢复）")]
        confirm: bool,
    },
    /// 查询计划任务
    Query {
        #[arg(
            long,
            default_value = "ZHFD-AutoCheckin",
            help = "Windows 计划任务名称"
        )]
        name: String,
    },
}

impl From<CliTaskCommand> for TaskRequest {
    fn from(command: CliTaskCommand) -> Self {
        match command {
            CliTaskCommand::Create {
                name,
                dry_run,
                live,
                confirm,
                all_instances,
                target,
            } => Self::Create {
                name,
                live,
                confirm,
                dry_run,
                all_instances,
                target: target.into(),
            },
            CliTaskCommand::Delete { name, confirm } => Self::Delete { name, confirm },
            CliTaskCommand::Query { name } => Self::Query { name },
        }
    }
}
#[derive(Subcommand, Debug)]
pub(crate) enum UiCommand {
    /// dump 当前 UI hierarchy（可能含账号/姓名等 PII）
    Dump {
        #[command(flatten)]
        target: CliInstanceTargetArgs,
    },
}
#[derive(Args, Debug, Clone)]
pub(crate) struct CliRunArgs {
    #[arg(
        long,
        conflicts_with = "live",
        help = "只观察，不点击签到按钮（首次推荐）"
    )]
    dry_run: bool,
    #[arg(
        long,
        requires = "confirm",
        help = "真实签到模式；必须同时传 --confirm（危险）"
    )]
    live: bool,
    #[arg(
        long,
        requires = "live",
        conflicts_with = "dry_run",
        help = "显式授权真实点击；只能与 --live 一起使用"
    )]
    confirm: bool,
    #[arg(
        long,
        conflicts_with_all = ["instance_index", "serial"],
        help = "依次运行配置中 enabled 的所有实例（不能与实例选择器组合）"
    )]
    all_instances: bool,
    #[arg(long, help = "只输出结果 JSON（便于脚本解析）")]
    pub(crate) json: bool,
    #[command(flatten)]
    pub(crate) target: CliInstanceTargetArgs,
}

impl CliRunArgs {
    pub(crate) fn into_request(self) -> RunRequest {
        let requested_mode = if self.live {
            RequestedMode::Live
        } else if self.dry_run {
            RequestedMode::DryRun
        } else {
            RequestedMode::FromConfig
        };
        RunRequest {
            requested_mode,
            explicit_confirmation: self.confirm,
            all_instances: self.all_instances,
            json: self.json,
            target: self.target.into(),
        }
    }
}

#[derive(Args, Debug, Clone)]
pub(crate) struct LoginArgs {
    #[arg(
        long,
        help = "显式确认在登录页执行一次真实点击（协议复选框 + 一键登录）"
    )]
    pub(crate) confirm_login: bool,
    #[arg(long, help = "只输出结果 JSON（便于脚本解析）")]
    pub(crate) json: bool,
    #[command(flatten)]
    pub(crate) target: CliInstanceTargetArgs,
}
