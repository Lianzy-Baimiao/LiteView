//! 开发用：按脚本回放操作并截图，用来检查界面（环境变量 `LITEVIEW_SCRIPT`，内容或文件路径）。
//! 每行一条：`wait 300`、`waitimage`、`key ctrl+r`、`move 100 200`、`click 100 200`、`rclick x y`、
//! `dclick x y`、`wheel -3`、`shot out.png`、`quit`。坐标是逻辑像素。

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
                if m.command && key == Key::C {
                    raw.events.push(Event::Copy);
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
            other => key = Key::from_name(other).or_else(|| Key::from_name(&other.to_ascii_uppercase())).or_else(|| Key::from_name(part)),
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
        assert!(matches!(parse_key("ArrowRight"), Some((_, Key::ArrowRight))));
    }
}
