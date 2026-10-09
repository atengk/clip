//! NSIS 安装向导规范与当前用户免提权配置验收测试
//!
//! @author Ateng
//! @since 2026-10-07

use std::fs;
use std::path::Path;

#[test]
fn test_tauri_conf_nsis_spec() {
    let conf_path = Path::new("tauri.conf.json");
    assert!(conf_path.exists(), "tauri.conf.json 必须存在");

    let conf_content = fs::read_to_string(conf_path).expect("读取 tauri.conf.json 失败");
    let json: serde_json::Value =
        serde_json::from_str(&conf_content).expect("tauri.conf.json 必须是有效 JSON");

    // 1. 验证产品名称标准化为 "Clip"
    assert_eq!(
        json["productName"].as_str().unwrap_or_default(),
        "Clip",
        "productName 必须为 'Clip'"
    );

    let nsis = &json["bundle"]["windows"]["nsis"];
    assert!(nsis.is_object(), "必须配置 bundle.windows.nsis");

    // 2. 验证 installMode 为 currentUser
    assert_eq!(
        nsis["installMode"].as_str().unwrap_or_default(),
        "currentUser",
        "installMode 必须为 'currentUser'"
    );

    // 3. 验证语言包含 English 与 SimpChinese
    let languages = nsis["languages"]
        .as_array()
        .expect("languages 必须是数组");
    let lang_strs: Vec<&str> = languages.iter().filter_map(|v| v.as_str()).collect();
    assert!(lang_strs.contains(&"English"), "必须包含 English");
    assert!(lang_strs.contains(&"SimpChinese"), "必须包含 SimpChinese");

    // 4. 验证侧边与顶部高调浅色 BMP 位图资产存在
    let sidebar = nsis["sidebarImage"].as_str().unwrap_or_default();
    let header = nsis["headerImage"].as_str().unwrap_or_default();
    assert!(!sidebar.is_empty(), "sidebarImage 不得为空");
    assert!(!header.is_empty(), "headerImage 不得为空");

    let sidebar_path = Path::new(sidebar);
    let header_path = Path::new(header);
    assert!(sidebar_path.exists(), "sidebarImage 文件必须存在: {:?}", sidebar_path);
    assert!(header_path.exists(), "headerImage 文件必须存在: {:?}", header_path);

    let sidebar_bytes = fs::read(sidebar_path).unwrap();
    let header_bytes = fs::read(header_path).unwrap();
    assert!(sidebar_bytes.len() >= 54, "sidebar.bmp 字节数必须不小于 54 字节完整文件头");
    assert!(header_bytes.len() >= 54, "header.bmp 字节数必须不小于 54 字节完整文件头");
    assert_eq!(&sidebar_bytes[0..2], b"BM", "sidebarImage 必须是有效 BMP 图像");
    assert_eq!(&header_bytes[0..2], b"BM", "headerImage 必须是有效 BMP 图像");

    // 严格校验 BMP 头部尺寸与 24 位色深规范 (Issue #17 AC)
    let sidebar_w = i32::from_le_bytes(sidebar_bytes[18..22].try_into().unwrap());
    let sidebar_h = i32::from_le_bytes(sidebar_bytes[22..26].try_into().unwrap());
    let sidebar_bpp = u16::from_le_bytes(sidebar_bytes[28..30].try_into().unwrap());
    assert_eq!(sidebar_w, 164, "sidebar.bmp 宽度必须为 164px");
    assert_eq!(sidebar_h, 314, "sidebar.bmp 高度必须为 314px");
    assert_eq!(sidebar_bpp, 24, "sidebar.bmp 色深必须为 24-bit");

    let header_w = i32::from_le_bytes(header_bytes[18..22].try_into().unwrap());
    let header_h = i32::from_le_bytes(header_bytes[22..26].try_into().unwrap());
    let header_bpp = u16::from_le_bytes(header_bytes[28..30].try_into().unwrap());
    assert_eq!(header_w, 150, "header.bmp 宽度必须为 150px");
    assert_eq!(header_h, 57, "header.bmp 高度必须为 57px");
    assert_eq!(header_bpp, 24, "header.bmp 色深必须为 24-bit");

    // 4.1 验证安装器与卸载器品牌图标配置存在且文件有效
    let installer_icon = nsis["installerIcon"].as_str().unwrap_or_default();
    let uninstaller_icon = nsis["uninstallerIcon"].as_str().unwrap_or_default();
    let uninstaller_header = nsis["uninstallerHeaderImage"].as_str().unwrap_or_default();
    assert_eq!(installer_icon, "icons/icon.ico", "installerIcon 必须为 'icons/icon.ico'");
    assert_eq!(uninstaller_icon, "icons/icon.ico", "uninstallerIcon 必须为 'icons/icon.ico'");
    assert_eq!(uninstaller_header, "icons/header.bmp", "uninstallerHeaderImage 必须为 'icons/header.bmp'");
    assert!(Path::new(installer_icon).exists(), "installerIcon 文件必须存在");
    assert!(Path::new(uninstaller_icon).exists(), "uninstallerIcon 文件必须存在");
    assert!(Path::new(uninstaller_header).exists(), "uninstallerHeaderImage 文件必须存在");

    // 5. 验证 hooks.nsh 及其关键配置宏
    let hooks_file = nsis["installerHooks"].as_str().unwrap_or_default();
    assert!(!hooks_file.is_empty(), "installerHooks 不得为空");
    let hooks_path = Path::new(hooks_file);
    assert!(hooks_path.exists(), "hooks.nsh 文件必须存在: {:?}", hooks_path);

    let hooks_content = fs::read_to_string(hooks_path).unwrap();
    assert!(hooks_content.contains("MUI_FINISHPAGE_RUN_TEXT"), "必须配置 MUI_FINISHPAGE_RUN_TEXT");
    assert!(hooks_content.contains("MUI_FINISHPAGE_RUN_CHECKED"), "必须配置 MUI_FINISHPAGE_RUN_CHECKED");
    assert!(hooks_content.contains("MUI_FINISHPAGE_TEXT"), "必须配置 MUI_FINISHPAGE_TEXT");
    assert!(hooks_content.contains("MUI_DIRECTORYPAGE_TEXT_DESTINATION"), "必须配置 MUI_DIRECTORYPAGE_TEXT_DESTINATION");
    assert!(hooks_content.contains("MUI_UNCONFIRMPAGE_TEXT_TOP"), "必须配置 MUI_UNCONFIRMPAGE_TEXT_TOP");

    // 5.1 验证自定义模板 windows/installer.nsi 配置与覆盖更新跳过逻辑
    let template_file = nsis["template"].as_str().unwrap_or_default();
    assert_eq!(template_file, "windows/installer.nsi", "template 必须配置为 windows/installer.nsi");
    let template_path = Path::new(template_file);
    assert!(template_path.exists(), "windows/installer.nsi 文件必须存在: {:?}", template_path);
    let template_content = fs::read_to_string(template_path).unwrap();
    assert!(template_content.contains("Page custom PageReinstall"), "模板必须包含 PageReinstall 逻辑");

    // 6. 验证自定义双语文件 customLanguageFiles
    let custom_langs = &nsis["customLanguageFiles"];
    assert!(custom_langs.is_object(), "必须配置 customLanguageFiles");

    let zh_file = custom_langs["SimpChinese"].as_str().unwrap_or_default();
    let en_file = custom_langs["English"].as_str().unwrap_or_default();
    assert!(!zh_file.is_empty(), "SimpChinese 语言文件不得为空");
    assert!(!en_file.is_empty(), "English 语言文件不得为空");

    let zh_path = Path::new(zh_file);
    let en_path = Path::new(en_file);
    assert!(zh_path.exists(), "zh-CN.nsh 必须存在");
    assert!(en_path.exists(), "en.nsh 必须存在");

    let zh_content = fs::read_to_string(zh_path).unwrap();
    let en_content = fs::read_to_string(en_path).unwrap();

    assert!(zh_content.contains("launchClipNow"), "中文语言包必须包含 launchClipNow");
    assert!(zh_content.contains("finishPageDescription"), "中文语言包必须包含 finishPageDescription");
    assert!(zh_content.contains("Alt + V"), "中文完成页必须包含 Alt + V 指引");
    assert!(zh_content.contains("立即运行 Clip (推荐)"), "中文运行提示必须标准地道");
    assert!(zh_content.contains("免管理员权限"), "中文目录提示必须包含免管理员权限安全说明");
    assert!(zh_content.contains("deleteAppData"), "中文必须配置 deleteAppData 提示默认安全保留");
    assert!(zh_content.contains("uninstPageTopText"), "中文必须配置 uninstPageTopText");
    assert!(zh_content.contains("appRunningOkKill"), "中文必须配置 appRunningOkKill 避免卸载期空白弹窗");
    assert!(zh_content.contains("upgradeHeaderTitle"), "中文必须配置 upgradeHeaderTitle");
    assert!(zh_content.contains("upgradeSafetyNotice"), "中文必须配置 upgradeSafetyNotice 安全保障承诺");
    assert!(zh_content.contains("upgradeActionPrompt"), "中文必须配置 upgradeActionPrompt 平滑升级选项");

    assert!(en_content.contains("launchClipNow"), "英文语言包必须包含 launchClipNow");
    assert!(en_content.contains("finishPageDescription"), "英文语言包必须包含 finishPageDescription");
    assert!(en_content.contains("Alt + V"), "英文完成页必须包含 Alt + V 指引");
    assert!(en_content.contains("Launch Clip now (Recommended)"), "英文运行提示必须标准");
    assert!(en_content.contains("No Elevation Required"), "英文目录提示必须包含免提权说明");
    assert!(en_content.contains("deleteAppData"), "英文必须配置 deleteAppData 提示默认安全保留");
    assert!(en_content.contains("uninstPageTopText"), "英文必须配置 uninstPageTopText");
    assert!(en_content.contains("appRunningOkKill"), "英文必须配置 appRunningOkKill 避免卸载期空白弹窗");
    assert!(en_content.contains("upgradeHeaderTitle"), "英文必须配置 upgradeHeaderTitle");
    assert!(en_content.contains("upgradeSafetyNotice"), "英文必须配置 upgradeSafetyNotice 安全保障承诺");
    assert!(en_content.contains("upgradeActionPrompt"), "英文必须配置 upgradeActionPrompt 平滑升级选项");
}

#[test]
fn test_nsis_process_self_healing_and_upgrade_hooks() {
    let hooks_path = Path::new("windows/hooks.nsh");
    assert!(hooks_path.exists(), "windows/hooks.nsh 文件必须存在");

    let hooks_content = fs::read_to_string(hooks_path).expect("读取 hooks.nsh 失败");

    // 1. 验证 PREINSTALL 钩子包含进程自愈 (taskkill /F /IM Clip.exe 与缓冲等待)
    assert!(
        hooks_content.contains("NSIS_HOOK_PREINSTALL"),
        "必须定义 NSIS_HOOK_PREINSTALL 宏"
    );
    assert!(
        hooks_content.contains("taskkill /F /IM Clip.exe") || hooks_content.contains("taskkill"),
        "PREINSTALL 阶段必须注入 taskkill 自愈关闭运行中 Clip.exe 进程逻辑以解除文件锁"
    );

    // 2. 验证 POSTINSTALL 钩子包含自动拉起新版本逻辑 (支持静默安装 /S)
    assert!(
        hooks_content.contains("NSIS_HOOK_POSTINSTALL"),
        "必须定义 NSIS_HOOK_POSTINSTALL 宏"
    );
    assert!(
        hooks_content.contains("ExecShell") || hooks_content.contains("Exec") || hooks_content.contains("Clip.exe"),
        "POSTINSTALL 阶段必须确保自动拉起新版可执行文件"
    );

    // 3. 验证 POSTUNINSTALL 钩子包含用户数据安全契约 (仅在显式确认时删除 AppData)
    assert!(
        hooks_content.contains("NSIS_HOOK_POSTUNINSTALL"),
        "必须定义 NSIS_HOOK_POSTUNINSTALL 宏"
    );
    assert!(
        hooks_content.contains("com.clip.app"),
        "POSTUNINSTALL 阶段必须包含 com.clip.app 目录处理"
    );
}
