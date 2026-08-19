#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(mdreader::run));
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => report_startup_failure(&format!("图形窗口初始化失败：{error}")),
        Err(payload) => {
            let detail = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("未知异常");
            report_startup_failure(&format!("应用启动时发生异常：{detail}"));
        }
    }
}

fn report_startup_failure(message: &str) {
    eprintln!("MD Reader failed to start: {message}");

    #[cfg(target_os = "windows")]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            let directory = std::path::PathBuf::from(local_app_data).join("MD Reader");
            let _ = std::fs::create_dir_all(&directory);
            let contents = format!("MD Reader {}\n{message}\n", env!("CARGO_PKG_VERSION"));
            let _ = std::fs::write(directory.join("startup-error.log"), contents);
        }
        let _ = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("MD Reader 无法启动")
            .set_description(format!(
                "{message}\n\n错误日志已写入 %LOCALAPPDATA%\\MD Reader\\startup-error.log"
            ))
            .show();
    }
}
