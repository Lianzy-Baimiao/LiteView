//! 开发用：按脚本回放操作并截图，用来检查界面（环境变量 `LITEVIEW_SCRIPT`，内容或文件路径）。
//! 每行一条：`wait 300`、`waitimage`、`key ctrl+r`、`move 100 200`、`click 100 200`、`rclick x y`、
//! `dclick x y`、`down x y`、`up`、`drag x1 y1 x2 y2 步数`、`wheel -3`、`shot out.png`、`quit`。坐标是逻辑像素。

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2};

#[derive(Debug)]
enum Step {
    Wait(Duration),
    WaitImage,
    Key(Modifiers, Key),
    Move(Pos2),
    Button(PointerButton, bool),
    Wheel(f32),
    Shot(String),
    Trace(String),
    Quit,
}

#[derive(Default)]
pub struct Script {
    steps: VecDeque<Step>,
    until: Option<Instant>,
    pos: Pos2,
    shot: Option<String>,
    image_ready: bool,
    first_frame: bool,
}

impl Script {
    pub fn from_env() -> Self {
        let Some(v) = std::env::var_os("LITEVIEW_SCRIPT") else { return Self::default() };
        let text = std::fs::read_to_string(&v).unwrap_or_else(|_| v.to_string_lossy().into_owned());
        Self { steps: parse(&text), ..Self::default() }
    }

    pub fn inject(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some(path) = &self.shot
            && let Some(img) = raw.events.iter().find_map(|e| match e {
                Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        {
            let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
            let r = image::save_buffer(path, &rgba, img.size[0] as u32, img.size[1] as u32, image::ColorType::Rgba8);
            log::info!("截图 {path}：{r:?}");
            self.shot = None;
        }
        if self.shot.is_some() || self.until.is_some_and(|t| Instant::now() < t) {
            ctx.request_repaint_after(Duration::from_millis(16));
            return;
        }
        self.until = None;
        let Some(step) = self.steps.pop_front() else { return };
        match step {
            Step::Wait(d) => self.until = Some(Instant::now() + d),
            Step::WaitImage => {
                if !self.image_ready {
                    self.steps.push_front(Step::WaitImage);
                }
            }
            Step::Key(m, key) => {
                raw.events.push(Event::ModifiersChanged(m));
                // 与 egui-winit 一致：Ctrl+C → Copy，Ctrl+X / Shift+Del → Cut（不再有按下的按键事件）
                if m.command && key == Key::C {
                    raw.events.push(Event::Copy);
                } else if (m.command && key == Key::X) || (cfg!(windows) && m.shift && key == Key::Delete) {
                    raw.events.push(Event::Cut);
                } else {
                    raw.events.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: m });
                }
                raw.events.push(Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: m });
            }
            Step::Move(p) => {
                self.pos = p;
                raw.events.push(Event::PointerMoved(p));
            }
            Step::Button(button, pressed) => {
                raw.events.push(Event::PointerButton { pos: self.pos, button, pressed, modifiers: Modifiers::NONE });
            }
            Step::Wheel(lines) => raw.events.push(Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, lines),
                phase: egui::TouchPhase::Move,
                modifiers: Modifiers::NONE,
            }),
            Step::Shot(path) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.shot = Some(path);
            }
            Step::Trace(label) => log::info!(
                "trace {label}：{:.0} ms，内存 {:.0} MB",
                crate::platform::ms_since_process_start(),
                crate::platform::memory_mb()
            ),
            Step::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
        ctx.request_repaint();
    }

    pub fn after_frame(&mut self, ctx: &egui::Context, image_ready: bool) {
        if !self.first_frame {
            self.first_frame = true;
            log::info!("启动 {:.0} ms：第一帧", crate::platform::ms_since_process_start());
        }
        self.image_ready = image_ready;
        if !self.steps.is_empty() || self.shot.is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }
}

fn parse(text: &str) -> VecDeque<Step> {
    let mut out = VecDeque::new();
    for line in text.lines().flat_map(|l| l.split(';')) {
        let w: Vec<&str> = line.split_whitespace().collect();
        let num = |i: usize| w.get(i).and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
        let at = || pos2(num(1), num(2));
        match w.first().copied() {
            Some("wait") => out.push_back(Step::Wait(Duration::from_millis(num(1) as u64))),
            Some("waitimage") => out.push_back(Step::WaitImage),
            Some("key") => {
                if let Some((m, k)) = w.get(1).and_then(|s| parse_key(s)) {
                    out.push_back(Step::Key(m, k));
                }
            }
            Some("move") => out.push_back(Step::Move(at())),
            // 按下 / 松开左键（down 带坐标时先移过去）
            Some("down") => {
                if w.len() >= 3 {
                    out.push_back(Step::Move(at()));
                }
                out.push_back(Step::Button(PointerButton::Primary, true));
            }
            Some("up") => out.push_back(Step::Button(PointerButton::Primary, false)),
            // drag x1 y1 x2 y2 [步数]：按住左键从一点拖到另一点
            Some("drag") => {
                let (a, b) = (pos2(num(1), num(2)), pos2(num(3), num(4)));
                let n = (num(5) as usize).max(1);
                out.push_back(Step::Move(a));
                out.push_back(Step::Button(PointerButton::Primary, true));
                for i in 1..=n {
                    out.push_back(Step::Move(a + (b - a) * (i as f32 / n as f32)));
                }
                out.push_back(Step::Button(PointerButton::Primary, false));
            }
            Some(c @ ("click" | "rclick" | "dclick")) => {
                let b = if c == "rclick" { PointerButton::Secondary } else { PointerButton::Primary };
                out.push_back(Step::Move(at()));
                for _ in 0..if c == "dclick" { 2 } else { 1 } {
                    out.push_back(Step::Button(b, true));
                    out.push_back(Step::Button(b, false));
                }
            }
            Some("wheel") => out.push_back(Step::Wheel(num(1))),
            Some("shot") => out.push_back(Step::Shot(w.get(1).unwrap_or(&"shot.png").to_string())),
            Some("trace") => out.push_back(Step::Trace(w.get(1).unwrap_or(&"").to_string())),
            Some("quit") => out.push_back(Step::Quit),
            _ => {}
        }
    }
    out
}

fn parse_key(s: &str) -> Option<(Modifiers, Key)> {
    let mut m = Modifiers::NONE;
    let mut key = None;
    for part in s.split('+') {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "cmd" => m.command = true,
            "shift" => m.shift = true,
            "alt" => m.alt = true,
            other => {
                key = Key::from_name(part).or_else(|| {
                    Key::ALL.iter().copied().find(|k| k.name().eq_ignore_ascii_case(other) || format!("{k:?}").eq_ignore_ascii_case(other) || k.symbol_or_name() == other)
                })
            }
        }
    }
    if m.command {
        m.ctrl = !cfg!(target_os = "macos");
        m.mac_cmd = cfg!(target_os = "macos");
    }
    key.map(|k| (m, k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_script() {
        let s = parse("wait 10; key ctrl+r\nclick 5 6\nshot a.png; quit");
        assert_eq!(s.len(), 7);
        assert!(matches!(s[1], Step::Key(m, Key::R) if m.command));
        assert!(matches!(parse_key("Escape"), Some((_, Key::Escape))));
        assert!(matches!(parse_key("escape"), Some((_, Key::Escape))));
        assert!(matches!(parse_key("enter"), Some((_, Key::Enter))));
        assert!(matches!(parse_key("ArrowRight"), Some((_, Key::ArrowRight))));
        assert!(matches!(parse_key("ctrl+shift+x"), Some((m, Key::X)) if m.command && m.shift));
        assert!(matches!(parse_key("["), Some((_, Key::OpenBracket))));
    }
}
