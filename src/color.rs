use blake3::Hasher;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug)]
struct Config {
    mode: ColorMode,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

fn parse_env_mode() -> Option<ColorMode> {
    // Respect OXPROC_COLOR if set
    if let Ok(v) = std::env::var("OXPROC_COLOR") {
        match v.to_lowercase().as_str() {
            "always" => return Some(ColorMode::Always),
            "never" => return Some(ColorMode::Never),
            "auto" => return Some(ColorMode::Auto),
            _ => {}
        }
    }
    // Respect NO_COLOR to force disable when not overridden
    if std::env::var_os("NO_COLOR").is_some() {
        return Some(ColorMode::Never);
    }
    None
}

pub fn init(mode_from_cli: Option<ColorMode>) {
    let mode = mode_from_cli
        .or_else(parse_env_mode)
        .unwrap_or(ColorMode::Auto);
    let _ = CONFIG.set(Config { mode });
}

fn current_mode() -> ColorMode {
    CONFIG.get().map(|c| c.mode).unwrap_or(ColorMode::Auto)
}

fn stdout_is_tty() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}

fn color_enabled() -> bool {
    match current_mode() {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => stdout_is_tty(),
    }
}

const PALETTE: [&str; 12] = [
    "\u{1b}[31m", // red
    "\u{1b}[32m", // green
    "\u{1b}[33m", // yellow
    "\u{1b}[34m", // blue
    "\u{1b}[35m", // magenta
    "\u{1b}[36m", // cyan
    "\u{1b}[91m", // bright red
    "\u{1b}[92m", // bright green
    "\u{1b}[93m", // bright yellow
    "\u{1b}[94m", // bright blue
    "\u{1b}[95m", // bright magenta
    "\u{1b}[96m", // bright cyan
];

pub const RESET: &str = "\u{1b}[0m";

fn color_index(label: &str) -> usize {
    let mut hasher = Hasher::new();
    hasher.update(label.as_bytes());
    let hash = hasher.finalize();
    // Take first 8 bytes for a u64, map to palette
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&hash.as_bytes()[0..8]);
    let v = u64::from_le_bytes(bytes);
    (v as usize) % PALETTE.len()
}

pub fn color_esc_for(label: &str) -> &'static str {
    let idx = color_index(label);
    PALETTE[idx]
}

pub fn prefix(label: &str) -> String {
    if color_enabled() {
        format!("[{}{}{}] ", color_esc_for(label), label, RESET)
    } else {
        format!("[{}] ", label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_deterministic() {
        let a = color_index("api");
        let b = color_index("api");
        assert_eq!(a, b);
    }

    #[test]
    fn prefix_shapes_colored() {
        init(Some(ColorMode::Always));
        let p = prefix("api");
        assert!(p.starts_with("["));
        assert!(p.ends_with("] "));
        assert!(p.contains(RESET));
    }
}
