//! LiteView 轻量看图：看图、编辑、打码。
//!
//! 模块：`decode` 解码（自带解码器 + 系统组件兜底）、`viewer` 看图（文件夹、预读、缩放、动图）、
//! `ui` 界面（主题、字体、图标、控件）、`app` 窗口与交互、`platform` 各系统专有的部分。

pub mod app;
pub mod cli;
pub mod clipboard;
pub mod decode;
pub mod devtools;
pub mod editor;
pub mod fsutil;
pub mod gpu;
pub mod i18n;
pub mod logger;
pub mod natsort;
pub mod platform;
pub mod save;
pub mod settings;
pub mod single;
pub mod ui;
pub mod viewer;

pub use app::run;
