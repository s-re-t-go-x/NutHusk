//! nuthusk-sign：NutHusk 插件签名工具。
//!
//!   nuthusk-sign keygen <名字>                       生成密钥对（<名字>.key 私钥，<名字>.pub 公钥）
//!   nuthusk-sign sign <插件包> --key <私钥文件> [--out <输出>] [--yes]
//!   nuthusk-sign verify <插件包> --keys <trusted_keys.json>
//!   nuthusk-sign sign-index <index.json> --key <私钥文件> [--out <输出>]

use nuthusk_core::package::{digest_of, inspect, write_signed};
use nuthusk_core::signature::{
    format_private_key, format_public_key, key_id_of, load_trusted_keys, parse_private_key, public_key_of_seed, sign,
    sign_index, TrustLevel,
};
use rand_core::{OsRng, RngCore};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const HELP: &str = "\
nuthusk-sign —— NutHusk 插件签名工具

用法：
  nuthusk-sign keygen <名字>
      生成密钥对：<名字>.key（私钥，务必保密）和 <名字>.pub（公钥，放进 trusted_keys.json）

  nuthusk-sign sign <插件包> --key <私钥文件> [--out <输出文件>] [--yes]
      审查并签名。会先列出插件的名称、权限、后端命令，请你确认已审查过再继续；
      输出默认为 <原文件名>.signed.<扩展名>，不会覆盖原文件。--yes 跳过确认（只用于自动化测试）

  nuthusk-sign verify <插件包> --keys <trusted_keys.json>
      按受信任公钥列表验证签名，打印结果（可信 / 未经安全验证 / 签名无效）

  nuthusk-sign sign-index <index.json> --key <私钥文件> [--out <输出文件>]
      给插件源的索引文件签名，默认输出 <index.json>.sig（放在 index.json 旁边一起发布）
";

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn run(args: Vec<String>) -> Result<(), String> {
    let cmd = args.first().map(String::as_str).unwrap_or("");
    match cmd {
        "keygen" => {
            let name = args.get(1).ok_or("用法: nuthusk-sign keygen <名字>")?;
            let (key_path, pub_path) = (PathBuf::from(format!("{name}.key")), PathBuf::from(format!("{name}.pub")));
            for p in [&key_path, &pub_path] {
                if p.exists() {
                    return Err(format!("{} 已存在，为避免覆盖已有密钥，请换个名字或先备份删除", p.display()));
                }
            }
            let mut seed = [0u8; 32];
            OsRng.fill_bytes(&mut seed);
            let public = public_key_of_seed(&seed);
            fs::write(&key_path, format!("{}\n", format_private_key(&seed))).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600));
            }
            fs::write(&pub_path, format!("{}\n", format_public_key(&public))).map_err(|e| e.to_string())?;
            println!("已生成：\n  私钥 {}（务必保密，不要提交到仓库，丢失后无法再给新插件签名）\n  公钥 {}", key_path.display(), pub_path.display());
            println!("密钥 ID：{}\n", key_id_of(&public));
            println!("把下面这段放进 src-tauri/trusted_keys.json 后重新编译，这个公钥签过名的插件就会显示为「可信来源」：");
            println!("{{\n  \"keys\": [\n    {{ \"name\": \"你的名字或组织\", \"public\": \"{}\" }}\n  ]\n}}", format_public_key(&public));
            Ok(())
        }
        "sign" => {
            let src = PathBuf::from(args.get(1).ok_or("用法: nuthusk-sign sign <插件包> --key <私钥文件>")?);
            let key_file = flag(&args, "--key").ok_or("缺少 --key <私钥文件>")?;
            let seed = parse_private_key(&fs::read_to_string(&key_file).map_err(|e| format!("无法读取私钥: {e}"))?)?;
            let dst = flag(&args, "--out").map(PathBuf::from).unwrap_or_else(|| default_out(&src));
            if dst == src {
                return Err("输出文件不能和原文件相同".into());
            }

            let (meta, digest) = digest_of(&src)?;
            let m = &meta.manifest;
            let info = inspect(&src, &[])?;
            println!("即将签名：{}  v{}  ({})", m.name, m.version, m.id);
            println!("  文件数 {}，解压后 {} 字节", info.file_count, info.unpacked_size);
            println!("  声明的权限：{}", if m.permissions.is_empty() { "无".to_string() } else { format!("{:?}", m.permissions) });
            println!("  后端程序：{}", info.backend_command.as_deref().unwrap_or("无（纯前端插件）"));
            println!("  内容摘要：{digest}");
            if !args.iter().any(|a| a == "--yes") {
                print!("\n签名表示你已审查过这个插件的全部代码并确认安全。已审查完毕？输入 yes 继续：");
                io::stdout().flush().ok();
                let mut line = String::new();
                io::stdin().read_line(&mut line).map_err(|e| e.to_string())?;
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
            let src = PathBuf::from(args.get(1).ok_or("用法: nuthusk-sign verify <插件包> --keys <trusted_keys.json>")?);
            let keys_file = flag(&args, "--keys").ok_or("缺少 --keys <trusted_keys.json>")?;
            let keys = load_trusted_keys(&fs::read_to_string(&keys_file).map_err(|e| format!("无法读取公钥列表: {e}"))?)?;
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
            let src = PathBuf::from(args.get(1).ok_or("用法: nuthusk-sign sign-index <index.json> --key <私钥文件>")?);
            let key_file = flag(&args, "--key").ok_or("缺少 --key <私钥文件>")?;
            let seed = parse_private_key(&fs::read_to_string(&key_file).map_err(|e| format!("无法读取私钥: {e}"))?)?;
            let bytes = fs::read(&src).map_err(|e| format!("无法读取索引: {e}"))?;
            serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|e| format!("索引不是有效的 JSON: {e}"))?;
            let dst = flag(&args, "--out").map(PathBuf::from).unwrap_or_else(|| {
                let mut name = src.file_name().map(|n| n.to_os_string()).unwrap_or_default();
                name.push(".sig");
                src.with_file_name(name)
            });
            let sig = sign_index(&seed, &bytes);
            fs::write(&dst, serde_json::to_string_pretty(&sig).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            println!("已签名：{}（密钥 ID {}）。索引改动后必须重新签名。", dst.display(), sig.key_id);
            Ok(())
        }
        "" | "help" | "--help" | "-h" => {
            print!("{HELP}");
            Ok(())
        }
        other => Err(format!("未知命令: {other}\n\n{HELP}")),
    }
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
