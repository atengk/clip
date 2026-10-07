//! 全平台核心引擎与发布打包全流程端到端回归验收测试 (Seams 1, 2, 3)
//!
//! @author Ateng
//! @since 2026-10-07

use std::fs;
use std::path::Path;
use std::sync::Arc;
use clip_lib::engine::privacy::{MaskedDetector, PrivacyFilter};
use clip_lib::engine::ClipboardEngine;
use clip_lib::pal::mock::MockPlatformDriver;
use clip_lib::pal::PlatformDriver;
use clip_lib::storage::sqlite::SqliteStorage;

#[test]
fn test_seam1_core_engine_and_ipc_deterministic_regression() {
    // 1. 初始化 MockPlatformDriver 与纯内存无头核心引擎 (毫秒级确定性)
    let mock_driver = Arc::new(MockPlatformDriver::new());
    let storage = Arc::new(SqliteStorage::new_in_memory().unwrap());
    let engine = Arc::new(ClipboardEngine::new(mock_driver.clone(), storage));

    // 2. 模拟常规文本写入与去重置顶
    mock_driver.write_text("你好世界，欢迎使用 Clip").unwrap();
    engine.handle_clipboard_change().unwrap().expect("捕获第一条记录");

    mock_driver.write_text("Rust + Tauri 极速桌面端开发").unwrap();
    engine.handle_clipboard_change().unwrap().expect("捕获第二条记录");

    // 3. 验证 FTS5 中文全文检索与拼音首字母过滤
    let search_res = engine.search_entries("nhsj", 10).expect("拼音首字母检索失败");
    assert_eq!(search_res.len(), 1, "必须检索到 '你好世界'");
    assert_eq!(search_res[0].content, "你好世界，欢迎使用 Clip");

    // 4. 模拟敏感信息脱敏与协议级 Clipboard Viewer Ignore 隐私隔离
    let raw_token = "ghp_123456789012345678901234567890123456";
    let (masked_token, detected) = MaskedDetector::mask_text(raw_token);
    assert!(detected, "必须成功嗅探到 API Token 凭据");
    assert_eq!(masked_token, "ghp_****3456", "必须符合 4 位首尾打码脱敏规则");

    // 协议级过滤验证 (密码管理器安全隔离，严禁入库落盘)
    let filter = PrivacyFilter::new();
    assert!(
        filter.should_ignore(true, None),
        "携带 Clipboard Viewer Ignore 标记时必须强制阻断忽略"
    );
    assert!(
        filter.should_ignore(false, Some("1Password.exe")),
        "密码管理器黑名单进程必须阻断忽略"
    );

    // 5. 模拟连贴队列生命周期
    let is_active = engine.toggle_paste_queue();
    assert!(is_active, "连贴队列启动激活");
    assert!(engine.get_paste_queue_status().is_active);

    mock_driver.write_text("队列项目1").unwrap();
    engine.handle_clipboard_change().unwrap();
    mock_driver.write_text("队列项目2").unwrap();
    engine.handle_clipboard_change().unwrap();

    let pop1 = engine.paste_queue_pop().expect("弹出队列第一项");
    assert_eq!(pop1.map(|i| i.content), Some("队列项目1".to_string()));
    let pop2 = engine.paste_queue_pop().expect("弹出队列第二项");
    assert_eq!(pop2.map(|i| i.content), Some("队列项目2".to_string()));
    assert_eq!(engine.paste_queue_pop().unwrap(), None);
    engine.toggle_paste_queue();

    // 6. 模拟常用短语模板与动态变量回填
    let snip = engine
        .save_snippet(None, "快捷问候", "你好，今天是 {year} 年！", "/hi")
        .expect("保存短语失败");
    let rendered = engine.paste_snippet(snip.id).expect("渲染短语失败");
    assert!(
        !rendered.contains("{year}") && rendered.contains("年！"),
        "变量 {{year}} 必须成功被真实年份展开替换: {}",
        rendered
    );

    // 7. 模拟清空历史与常用短语安全隔离
    engine.clear_history().expect("清空历史记录失败");
    assert_eq!(engine.get_entries(10).unwrap().len(), 0);
    let snippets = engine.get_all_snippets().unwrap();
    assert!(
        snippets.iter().any(|s| s.title == "快捷问候"),
        "清空历史不影响常用短语模板库"
    );
}

#[test]
fn test_seam2_frontend_compilation_and_assets_existence() {
    // 验证前端编译构建目录及核心资源 (支持已构建环境严格断言)
    let dist_dir = Path::new("../dist");
    let index_html = dist_dir.join("index.html");
    if !dist_dir.exists() {
        eprintln!("注意: 前端未预先构建，跳过 dist/ 静态产物物理存在性验证");
        return;
    }
    assert!(index_html.exists(), "dist/index.html 必须存在");

    let html_content = fs::read_to_string(&index_html).expect("读取 index.html 失败");
    assert!(html_content.contains("<!doctype html>"), "index.html 格式必须为标准 HTML5");
    assert!(html_content.contains("<div id=\"root\"></div>"), "必须包含 React 挂载根节点");
}

#[test]
fn test_seam3_conformance_and_zero_secret_invariant() {
    // 1. 验证 tauri.conf.json 基础规范
    let conf_path = Path::new("tauri.conf.json");
    let conf_str = fs::read_to_string(conf_path).expect("读取 tauri.conf.json 失败");
    assert!(conf_str.contains("\"productName\": \"Clip\""), "必须为 Clip");
    assert!(conf_str.contains("\"installMode\": \"currentUser\""), "必须为 currentUser");

    // 2. 验证 release.yml 脚本规范性与防注入
    let release_yml_path = Path::new("../.github/workflows/release.yml");
    assert!(release_yml_path.exists(), "release.yml 必须存在");
    let release_yml = fs::read_to_string(release_yml_path).expect("读取 release.yml 失败");
    assert!(release_yml.contains("TAG_REF: ${{ github.ref_name }}"), "必须通过 TAG_REF 防范注入");
    assert!(
        release_yml.contains("sha256sum \"${normalized_files[@]}\" > checksums.txt"),
        "必须安全编译 SHA-256 清单"
    );
    assert!(release_yml.contains("sha256sum -c checksums.txt"), "必须包含清单自检");

    // 3. 验证仓库无明文私钥或违规敏感信息
    assert!(!conf_str.contains("sk-"), "配置文件不得包含真实私钥");
    assert!(!conf_str.contains("AIzaSy"), "配置文件不得包含真实 Google API 凭据");
}
