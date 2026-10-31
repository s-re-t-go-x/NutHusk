//! nuthusk-cli：NutHusk 插件开发、打包、校验与签名工具。
//!
//!   nuthusk-cli new <插件id> [--lang <js|python|rust|ui-only>] [--name <显示名>] [--dir <输出父目录>]
//!   nuthusk-cli check <插件目录> [--no-backend]
//!   nuthusk-cli pack <插件目录> [--out <输出文件.nut>]
//!   nuthusk-cli run <插件目录> <动作id或方法名> [--input <文本>]
//!   nuthusk-cli keygen <名字>                       生成密钥对（<名字>.key 私钥，<名字>.pub 公钥）
//!   nuthusk-cli sign <插件包> --key <私钥文件> [--out <输出>] [--yes]
//!   nuthusk-cli verify <插件包> --keys <trusted_keys.json>
//!   nuthusk-cli sign-index <index.json> --key <私钥文件> [--out <输出>]

mod config;

use nuthusk_core::devkit::{
    check_plugin_dir, pack_plugin_dir, run_headless_action, scaffold_plugin,
};
use nuthusk_core::package::{digest_of, inspect, write_signed};
use nuthusk_core::signature::{
    format_private_key, format_public_key, key_id_of, load_trusted_keys, parse_private_key,
    public_key_of_seed, sign, sign_index, TrustLevel,
};
use rand_core::{OsRng, RngCore};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const HELP: &str = "\
nuthusk-cli —— NutHusk 插件开发、校验、打包与签名工具

开发与调试命令：
  nuthusk-cli new <插件id> [--lang <js|python|rust|ui-only>] [--name <显示名>] [--dir <输出目录>]
      生成插件脚手架目录（默认语言为 js），包含 plugin.json、icon.svg、ui.html 及对应后端骨架

  nuthusk-cli check <插件目录> [--no-backend]
      校验 plugin.json 清单、安全路径、图标规格，并启动一次后端进程测试 stdin/stdout JSON-RPC 2.0 响应

  nuthusk-cli pack <插件目录> [--out <输出文件.nut>]
      校验插件目录后直接打包为可安装的 .nut 插件包

  nuthusk-cli run <插件目录> <动作id或方法名> [--input <文本>]
      无界面直接运行插件动作（不传 --input 时从标准输入读取文本），结果输出到标准输出

配置生成命令：
  nuthusk-cli index <插件包目录> --base-url <下载目录URL> [--name <源名称>] [--author <作者>] [--out index.json] [--force]
      从最终发布包读取清单，自动生成 ID、版本、SHA-256、大小和下载地址；每个 ID 只允许一个包

  nuthusk-cli runtime-recipe <名字> --version <版本> --file <本地安装包> --url <下载URL> --bin <可执行文件相对路径>
      [--platform <系统-架构>] [--archive <zip|tar.gz|file>] [--strip-components <层数>]
      [--keep <逗号分隔的文件>] [--aliases <逗号分隔的别名>] [--description <说明>] [--out <配方.json>] [--force]
      从本地文件自动计算 SHA-256 和大小，生成可放入 nutConfig/runtimes.d/ 的配方；默认平台为当前平台
      两个命令均离线生成，默认不覆盖已有输出；--force 会完整重建输出文件

签名与安全命令：
  nuthusk-cli keygen <名字>
      生成密钥对：<名字>.key（私钥，务必保密）和 <名字>.pub（公钥，放进 trusted_keys.json）

  nuthusk-cli sign <插件包> --key <私钥文件> [--out <输出文件>] [--yes]
      审查并签名。会先列出插件的名称、权限、后端命令，请你确认已审查过再继续；
      输出默认为 <原文件名>.signed.<扩展名>，不会覆盖原文件。--yes 跳过确认（只用于自动化测试）

  nuthusk-cli verify <插件包> --keys <trusted_keys.json>
      按受信任公钥列表验证签名，打印结果（可信 / 未经安全验证 / 签名无效）

  nuthusk-cli sign-index <index.json> --key <私钥文件> [--out <输出文件>]
      给插件源的索引文件签名，默认输出 <index.json>.sig（放在 index.json 旁边一起发布）
";

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn run(args: Vec<String>) -> Result<(), String> {
    let cmd = args.first().map(String::as_str).unwrap_or("");
    match cmd {
        "index" => config::index(&args),
        "runtime-recipe" => config::runtime_recipe(&args),
        "new" => {
            let id = args.get(1).ok_or("用法: nuthusk-cli new <插件id> [--lang <js|python|rust|ui-only>] [--name <显示名>]")?;
            let lang = flag(&args, "--lang").unwrap_or_else(|| "js".into());
            let name = flag(&args, "--name");
            let base = flag(&args, "--dir")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            let created = scaffold_plugin(&base, id, name.as_deref(), &lang)?;
            println!("已创建插件脚手架：{}（模板：{}）", created.display(), lang);
            println!(
                "下一步：\n  1. 校验协议：nuthusk-cli check {}\n  2. 打包插件：nuthusk-cli pack {}",
                created.display(),
                created.display()
            );
            Ok(())
        }
        "check" => {
            let dir = PathBuf::from(
                args.get(1)
                    .ok_or("用法: nuthusk-cli check <插件目录> [--no-backend]")?,
            );
            let test_backend = !args.iter().any(|a| a == "--no-backend");
            let rep = check_plugin_dir(&dir, test_backend)?;
            let m = &rep.manifest;
            println!("✓ 清单与目录校验通过：{} v{} ({})", m.name, m.version, m.id);
            println!(
                "  文件数：{}，总大小：{} 字节",
                rep.file_count, rep.total_bytes
            );
            println!("  声明动作数：{}", m.actions.len());
            if let Some(cmd_str) = &rep.backend_command {
                if rep.backend_tested {
                    println!("✓ 后端 JSON-RPC 协议探测通过（{}）", cmd_str);
                } else {
                    println!("  后端命令：{}（已跳过进程探测）", cmd_str);
                }
            } else {
                println!("  类型：纯前端插件（无后端进程）");
            }
            Ok(())
        }
        "pack" => {
            let dir = PathBuf::from(
                args.get(1)
                    .ok_or("用法: nuthusk-cli pack <插件目录> [--out <输出文件.nut>]")?,
            );
            let out = flag(&args, "--out").map(PathBuf::from);
            let dst = pack_plugin_dir(&dir, out.as_deref())?;
            println!("已打包插件：{}", dst.display());
            Ok(())
        }
        "run" => {
            let dir = PathBuf::from(
                args.get(1)
                    .ok_or("用法: nuthusk-cli run <插件目录> <动作id或方法名> [--input <文本>]")?,
            );
            let action = args
                .get(2)
                .ok_or("用法: nuthusk-cli run <插件目录> <动作id或方法名> [--input <文本>]")?;
            let input_text = match flag(&args, "--input") {
                Some(s) => s,
                None => {
                    let mut buf = String::new();
                    io::stdin()
                        .read_to_string(&mut buf)
                        .map_err(|e| format!("读取标准输入失败: {e}"))?;
                    buf
                }
            };
            let res = run_headless_action(&dir, action, &input_text, None)?;
            match res {
                serde_json::Value::String(s) => println!("{s}"),
                other => println!(
                    "{}",
                    serde_json::to_string_pretty(&other).unwrap_or_else(|_| other.to_string())
                ),
            }
            Ok(())
        }
        "keygen" => {
            let name = args.get(1).ok_or("用法: nuthusk-cli keygen <名字>")?;
            let (key_path, pub_path) = (
                PathBuf::from(format!("{name}.key")),
                PathBuf::from(format!("{name}.pub")),
            );
            for p in [&key_path, &pub_path] {
                if p.exists() {
                    return Err(format!(
                        "{} 已存在，为避免覆盖已有密钥，请换个名字或先备份删除",
                        p.display()
                    ));
                }
            }
            let mut seed = [0u8; 32];
            OsRng.fill_bytes(&mut seed);
            let public = public_key_of_seed(&seed);
            write_new_key_file(&key_path, &format!("{}\n", format_private_key(&seed)), true)?;
            write_new_key_file(&pub_path, &format!("{}\n", format_public_key(&public)), false)?;
            println!("已生成：\n  私钥 {}（务必保密，不要提交到仓库，丢失后无法再给新插件签名）\n  公钥 {}", key_path.display(), pub_path.display());
            println!("密钥 ID：{}\n", key_id_of(&public));
            println!("把下面这段放进 src-tauri/trusted_keys.json 后重新编译，这个公钥签过名的插件就会显示为「可信来源」：");
            println!("{{\n  \"keys\": [\n    {{ \"name\": \"你的名字或组织\", \"public\": \"{}\" }}\n  ]\n}}", format_public_key(&public));
            Ok(())
        }
        "sign" => {
            let src = PathBuf::from(
                args.get(1)
                    .ok_or("用法: nuthusk-cli sign <插件包> --key <私钥文件>")?,
            );
            let key_file = flag(&args, "--key").ok_or("缺少 --key <私钥文件>")?;
            let seed = parse_private_key(
                &fs::read_to_string(&key_file).map_err(|e| format!("无法读取私钥: {e}"))?,
            )?;
            let dst = flag(&args, "--out")
                .map(PathBuf::from)
                .unwrap_or_else(|| default_out(&src));
            if dst == src {
                return Err("输出文件不能和原文件相同".into());
            }

            let (meta, digest) = digest_of(&src)?;
            let m = &meta.manifest;
            let info = inspect(&src, &[])?;
            println!("即将签名：{}  v{}  ({})", m.name, m.version, m.id);
            println!(
                "  文件数 {}，解压后 {} 字节",
                info.file_count, info.unpacked_size
            );
            println!(
                "  声明的权限：{}",
                if m.permissions.is_empty() {
                    "无".to_string()
                } else {
                    format!("{:?}", m.permissions)
                }
            );
            println!(
                "  后端程序：{}",
                info.backend_command
                    .as_deref()
                    .unwrap_or("无（纯前端插件）")
            );
            println!("  内容摘要：{digest}");
            if !args.iter().any(|a| a == "--yes") {
                print!(
                    "\n签名表示你已审查过这个插件的全部代码并确认安全。已审查完毕？输入 yes 继续："
                );
                io::stdout().flush().ok();
                let mut line = String::new();
                io::stdin()
                    .read_line(&mut line)
                    .map_err(|e| e.to_string())?;
                if line.trim() != "yes" {
                    return Err("已取消，没有签名".into());
                }
            }
            let sig = sign(&seed, &m.id, &m.version, &digest);
            let json = serde_json::to_string_pretty(&sig).map_err(|e| e.to_string())?;
            write_signed(&src, &dst, &json)?;
            println!("\n已签名：{}（密钥 ID {}）", dst.display(), sig.key_id);
            Ok(())
        }
        "verify" => {
            let src = PathBuf::from(
                args.get(1)
                    .ok_or("用法: nuthusk-cli verify <插件包> --keys <trusted_keys.json>")?,
            );
            let keys_file = flag(&args, "--keys").ok_or("缺少 --keys <trusted_keys.json>")?;
            let keys = load_trusted_keys(
                &fs::read_to_string(&keys_file).map_err(|e| format!("无法读取公钥列表: {e}"))?,
            )?;
            let info = inspect(&src, &keys)?;
            let label = match info.trust.level {
                TrustLevel::Trusted => "可信来源",
                TrustLevel::Unverified => "未经安全验证",
                TrustLevel::Invalid => "签名无效",
            };
            println!("{label}：{}", info.trust.reason);
            if info.trust.level == TrustLevel::Invalid {
                return Err("签名无效".into());
            }
            Ok(())
        }
        "sign-index" => {
            let src = PathBuf::from(
                args.get(1)
                    .ok_or("用法: nuthusk-cli sign-index <index.json> --key <私钥文件>")?,
            );
            let key_file = flag(&args, "--key").ok_or("缺少 --key <私钥文件>")?;
            let seed = parse_private_key(
                &fs::read_to_string(&key_file).map_err(|e| format!("无法读取私钥: {e}"))?,
            )?;
            let bytes = fs::read(&src).map_err(|e| format!("无法读取索引: {e}"))?;
            serde_json::from_slice::<serde_json::Value>(&bytes)
                .map_err(|e| format!("索引不是有效的 JSON: {e}"))?;
            let dst = flag(&args, "--out").map(PathBuf::from).unwrap_or_else(|| {
                let mut name = src
                    .file_name()
                    .map(|n| n.to_os_string())
                    .unwrap_or_default();
                name.push(".sig");
                src.with_file_name(name)
            });
            let sig = sign_index(&seed, &bytes);
            fs::write(
                &dst,
                serde_json::to_string_pretty(&sig).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            println!(
                "已签名：{}（密钥 ID {}）。索引改动后必须重新签名。",
                dst.display(),
                sig.key_id
            );
            Ok(())
        }
        "" | "help" | "--help" | "-h" => {
            print!("{HELP}");
            Ok(())
        }
        other => Err(format!("未知命令: {other}\n\n{HELP}")),
    }
}

/// Create private material with restrictive permissions from the first write, never follow a pre-existing symlink.
fn write_new_key_file(path: &Path, contents: &str, private: bool) -> Result<(), String> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if private { 0o600 } else { 0o644 });
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut file = options.open(path).map_err(|e| format!("无法创建 {}: {e}", path.display()))?;
    file.write_all(contents.as_bytes()).and_then(|_| file.sync_all())
        .map_err(|e| format!("无法保存 {}: {e}", path.display()))
}

fn default_out(src: &Path) -> PathBuf {
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("plugin");
    let ext = src.extension().and_then(|s| s.to_str()).unwrap_or("nut");
    src.with_file_name(format!("{stem}.signed.{ext}"))
}

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("错误：{e}");
            ExitCode::FAILURE
        }
    }
}
