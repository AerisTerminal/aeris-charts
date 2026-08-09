use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn required_string<'a>(value: &'a Value, path: &[&str]) -> &'a str {
    let mut current = value;
    for key in path {
        current = current
            .get(key)
            .unwrap_or_else(|| panic!("missing style token `{}`", path.join(".")));
    }
    current
        .as_str()
        .unwrap_or_else(|| panic!("style token `{}` must be a string", path.join(".")))
}

fn required_u8(value: &Value, path: &[&str]) -> u8 {
    let mut current = value;
    for key in path {
        current = current
            .get(key)
            .unwrap_or_else(|| panic!("missing style token `{}`", path.join(".")));
    }
    let number = current
        .as_u64()
        .unwrap_or_else(|| panic!("style token `{}` must be an integer", path.join(".")));
    u8::try_from(number)
        .unwrap_or_else(|_| panic!("style token `{}` must fit in one byte", path.join(".")))
}

fn rgb(css: &str, name: &str) -> (u8, u8, u8) {
    let hex = css
        .strip_prefix('#')
        .unwrap_or_else(|| panic!("style token `{name}` must use #rrggbb syntax"));
    assert_eq!(hex.len(), 6, "style token `{name}` must use #rrggbb syntax");
    let byte = |start| {
        u8::from_str_radix(&hex[start..start + 2], 16)
            .unwrap_or_else(|_| panic!("style token `{name}` must use #rrggbb syntax"))
    };
    (byte(0), byte(2), byte(4))
}

fn emit_color(output: &mut String, name: &str, css: &str) {
    let (red, green, blue) = rgb(css, name);
    output.push_str(&format!("pub const {name}_CSS: &str = \"{css}\";\n"));
    output.push_str(&format!(
        "pub const {name}_RGB: (u8, u8, u8) = (0x{red:02x}, 0x{green:02x}, 0x{blue:02x});\n"
    ));
}

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let tokens_path = manifest_dir.join("../../packages/charts/src/style_tokens.json");
    println!("cargo:rerun-if-changed={}", tokens_path.display());

    let source = fs::read_to_string(&tokens_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", tokens_path.display()));
    let tokens: Value = serde_json::from_str(&source)
        .unwrap_or_else(|error| panic!("invalid {}: {error}", tokens_path.display()));
    let default_theme = required_string(&tokens, &["default_theme"]);
    assert!(
        matches!(default_theme, "light" | "dark"),
        "default_theme must be light or dark"
    );

    let mut output = String::from("// Generated from packages/charts/src/style_tokens.json.\n");
    output.push_str(&format!(
        "pub const DEFAULT_THEME_NAME: &str = \"{default_theme}\";\n"
    ));
    for theme in ["light", "dark"] {
        let prefix = theme.to_ascii_uppercase();
        for (field, suffix) in [
            ("surface", "SURFACE"),
            ("border", "BORDER"),
            ("axis_text", "AXIS_TEXT"),
            ("crosshair", "CROSSHAIR"),
        ] {
            emit_color(
                &mut output,
                &format!("{prefix}_{suffix}"),
                required_string(&tokens, &[theme, field]),
            );
        }
    }
    emit_color(
        &mut output,
        "MARKET_UP",
        required_string(&tokens, &["market", "up"]),
    );
    emit_color(
        &mut output,
        "MARKET_DOWN",
        required_string(&tokens, &["market", "down"]),
    );
    let volume_alpha = required_u8(&tokens, &["market", "volume_alpha"]);
    output.push_str(&format!(
        "pub const MARKET_VOLUME_ALPHA: u8 = 0x{volume_alpha:02x};\n"
    ));

    let default_prefix = default_theme.to_ascii_uppercase();
    for suffix in ["SURFACE", "BORDER", "AXIS_TEXT", "CROSSHAIR"] {
        output.push_str(&format!(
            "pub const DEFAULT_{suffix}_CSS: &str = {default_prefix}_{suffix}_CSS;\n"
        ));
        output.push_str(&format!(
            "pub const DEFAULT_{suffix}_RGB: (u8, u8, u8) = {default_prefix}_{suffix}_RGB;\n"
        ));
    }

    let output_path = Path::new(&env::var_os("OUT_DIR").expect("out dir")).join("style_tokens.rs");
    fs::write(&output_path, output)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", output_path.display()));
}
