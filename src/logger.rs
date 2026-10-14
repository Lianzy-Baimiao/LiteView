//! 简单日志：debug 构建写到 stderr；设置了环境变量 `LITEVIEW_LOG=<文件>` 时追加写到文件。
//! `LITEVIEW_LOG_LEVEL` 可设 error / warn / info / debug / trace（默认 info）。

use std::fs::File;
use std::io::Write;
use std::sync::Mutex;
use std::time::Instant;

use log::{Level, LevelFilter, Log, Metadata, Record};

struct Logger {
    file: Option<Mutex<File>>,
    start: Instant,
}

impl Log for Logger {
    fn enabled(&self, _: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if !record.target().starts_with("liteview") && record.level() > Level::Warn {
            return;
        }
        // 加载系统字体时，读不了的字体文件每个都会警告一次（不影响使用）
        if record.target().starts_with("fontdb") && record.level() > Level::Error {
            return;
        }
        let line = format!(
            "[{:>8.1} ms {:5} {}] {}\n",
            self.start.elapsed().as_secs_f64() * 1000.0,
            record.level(),
            record.target(),
            record.args()
        );
        if cfg!(debug_assertions) {
            eprint!("{line}");
        }
        if let Some(f) = &self.file
            && let Ok(mut f) = f.lock()
        {
            let _ = f.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {}
}

pub fn init() {
    let file = std::env::var_os("LITEVIEW_LOG")
        .and_then(|p| File::options().create(true).append(true).open(p).ok())
        .map(Mutex::new);
    let level = match std::env::var("LITEVIEW_LOG_LEVEL").as_deref() {
        Ok("error") => LevelFilter::Error,
        Ok("warn") => LevelFilter::Warn,
        Ok("debug") => LevelFilter::Debug,
        Ok("trace") => LevelFilter::Trace,
        _ => LevelFilter::Info,
    };
    let logger = Logger { file, start: Instant::now() };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(level);
    }
}
