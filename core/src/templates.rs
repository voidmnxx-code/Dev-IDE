//! Starter projects: static web, and Rust 2D games that compile to WebAssembly.

use crate::IdeError;
use std::path::Path;

pub struct Template {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// (relative path, contents). `{{name}}` = project name, `{{crate}}` = valid crate/identifier name.
    pub files: &'static [(&'static str, &'static str)],
}

const WEB: Template = Template {
    id: "web",
    name: "Web page",
    description: "HTML + CSS + JavaScript with live reload",
    files: &[
        ("index.html", "<!doctype html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n  <title>{{name}}</title>\n  <link rel=\"stylesheet\" href=\"style.css\">\n</head>\n<body>\n  <h1>{{name}}</h1>\n  <button id=\"btn\">Click me</button>\n  <p id=\"out\"></p>\n  <script src=\"main.js\"></script>\n</body>\n</html>\n"),
        ("style.css", "body { font-family: system-ui, sans-serif; margin: 2rem; }\nbutton { font-size: 1.2rem; padding: .6rem 1rem; }\n"),
        ("main.js", "let n = 0;\ndocument.getElementById('btn').addEventListener('click', () => {\n  n += 1;\n  document.getElementById('out').textContent = 'Clicked ' + n + ' times';\n  console.log('clicked', n);\n});\n"),
    ],
};

const MACROQUAD: Template = Template {
    id: "macroquad",
    name: "Macroquad game (2D)",
    description: "Simple game loop, builds to WebAssembly",
    files: &[
        ("Cargo.toml", "[package]\nname = \"{{crate}}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nmacroquad = \"0.4\"\n"),
        ("src/main.rs", "use macroquad::prelude::*;\n\n#[macroquad::main(\"{{name}}\")]\nasync fn main() {\n    let mut pos = vec2(screen_width() / 2.0, screen_height() / 2.0);\n    loop {\n        clear_background(BLACK);\n        let speed = 300.0 * get_frame_time();\n        if is_key_down(KeyCode::Right) { pos.x += speed; }\n        if is_key_down(KeyCode::Left) { pos.x -= speed; }\n        if is_key_down(KeyCode::Down) { pos.y += speed; }\n        if is_key_down(KeyCode::Up) { pos.y -= speed; }\n        // Touch / mouse: move toward the pointer\n        if is_mouse_button_down(MouseButton::Left) {\n            let (mx, my) = mouse_position();\n            pos += (vec2(mx, my) - pos).clamp_length_max(speed);\n        }\n        draw_circle(pos.x, pos.y, 24.0, SKYBLUE);\n        draw_text(\"{{name}}\", 20.0, 40.0, 32.0, WHITE);\n        next_frame().await\n    }\n}\n"),
        ("index.html", "<!doctype html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n  <title>{{name}}</title>\n  <style>html,body,canvas{margin:0;padding:0;width:100%;height:100%;overflow:hidden;background:#000}</style>\n</head>\n<body>\n  <canvas id=\"glcanvas\" tabindex=\"1\"></canvas>\n  <!-- For offline use, download this file next to index.html and change the src. -->\n  <script src=\"https://not-fl3.github.io/miniquad-samples/mq_js_bundle.js\"></script>\n  <script>load(\"target/wasm32-unknown-unknown/release/{{crate}}.wasm\");</script>\n</body>\n</html>\n"),
        ("build.sh", "#!/bin/sh\nset -e\ncargo build --release --target wasm32-unknown-unknown\n"),
        (".ide-preview.json", "{ \"isolated\": false }\n"),
    ],
};

const BEVY: Template = Template {
    id: "bevy",
    name: "Bevy game (2D)",
    description: "ECS game for WebAssembly (not verified yet: needs rustc 1.97.1+, long first build)",
    files: &[
        ("Cargo.toml", "[package]\nname = \"{{crate}}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nbevy = \"0.20\"\n\n# Faster incremental builds while developing.\n[profile.dev]\nopt-level = 1\n\n[profile.dev.package.\"*\"]\nopt-level = 3\n"),
        ("src/main.rs", "use bevy::prelude::*;\n\nfn main() {\n    App::new()\n        .add_plugins(DefaultPlugins)\n        .add_systems(Startup, setup)\n        .add_systems(Update, spin)\n        .run();\n}\n\n#[derive(Component)]\nstruct Spinner;\n\nfn setup(mut commands: Commands) {\n    commands.spawn(Camera2d);\n    commands.spawn((\n        Sprite::from_color(Color::srgb(0.25, 0.6, 1.0), Vec2::splat(120.0)),\n        Transform::default(),\n        Spinner,\n    ));\n}\n\nfn spin(time: Res<Time>, mut q: Query<&mut Transform, With<Spinner>>) {\n    for mut t in &mut q {\n        t.rotate_z(time.delta_secs());\n    }\n}\n"),
        ("index.html", "<!doctype html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n  <title>{{name}}</title>\n  <style>html,body{margin:0;height:100%;background:#000}canvas{width:100%;height:100%}</style>\n</head>\n<body>\n  <script type=\"module\">\n    import init from './web/{{crate}}.js';\n    init();\n  </script>\n</body>\n</html>\n"),
        ("build.sh", "#!/bin/sh\n# Needs: rustup target add wasm32-unknown-unknown ; cargo install wasm-bindgen-cli\nset -e\ncargo build --release --target wasm32-unknown-unknown\nwasm-bindgen --out-dir web --target web target/wasm32-unknown-unknown/release/{{crate}}.wasm\n"),
        (".ide-preview.json", "{ \"isolated\": false }\n"),
    ],
};

pub const ALL: &[&Template] = &[&WEB, &MACROQUAD, &BEVY];

/// Valid Rust crate / JS identifier derived from a project name.
pub fn crate_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    if s.is_empty() || s.chars().next().unwrap().is_ascii_digit() {
        s.insert(0, 'p');
    }
    s
}

/// Create `<root>/<name>/` from template `id`. Returns the project's path relative to `root`.
pub fn create(root: &Path, id: &str, name: &str) -> Result<String, IdeError> {
    let t = ALL
        .iter()
        .find(|t| t.id == id)
        .ok_or_else(|| IdeError::Io(format!("unknown template '{id}'")))?;
    let name = name.trim();
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        return Err(IdeError::Io(format!("invalid project name '{name}'")));
    }
    let dir = root.join(name);
    if dir.exists() {
        return Err(IdeError::AlreadyExists(name.to_string()));
    }
    let krate = crate_name(name);
    for (rel, body) in t.files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap())?;
        std::fs::write(&p, body.replace("{{name}}", name).replace("{{crate}}", &krate))?;
        #[cfg(unix)]
        if rel.ends_with(".sh") {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_every_template_with_substitution() {
        for t in ALL {
            let tmp = tempfile::tempdir().unwrap();
            let rel = create(tmp.path(), t.id, "My Game!").unwrap();
            assert_eq!(rel, "My Game!");
            for (f, _) in t.files {
                let body = std::fs::read_to_string(tmp.path().join("My Game!").join(f)).unwrap();
                assert!(!body.contains("{{"), "{} / {f} has an unreplaced placeholder", t.id);
            }
        }
        let tmp = tempfile::tempdir().unwrap();
        create(tmp.path(), "macroquad", "Space Run").unwrap();
        let toml = std::fs::read_to_string(tmp.path().join("Space Run/Cargo.toml")).unwrap();
        assert!(toml.contains("name = \"space_run\""));
        let html = std::fs::read_to_string(tmp.path().join("Space Run/index.html")).unwrap();
        assert!(html.contains("release/space_run.wasm"));
    }

    #[test]
    fn rejects_bad_input() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(create(tmp.path(), "nope", "x").is_err());
        assert!(create(tmp.path(), "web", "").is_err());
        assert!(create(tmp.path(), "web", "../evil").is_err());
        create(tmp.path(), "web", "a").unwrap();
        assert!(matches!(create(tmp.path(), "web", "a"), Err(IdeError::AlreadyExists(_))));
    }

    #[test]
    fn crate_names_are_valid() {
        assert_eq!(crate_name("My-Game 2"), "my_game_2");
        assert_eq!(crate_name("3d"), "p3d");
        assert_eq!(crate_name(""), "p");
    }

    #[test]
    fn game_templates_are_detected_as_games_and_previewable() {
        let tmp = tempfile::tempdir().unwrap();
        create(tmp.path(), "macroquad", "g").unwrap();
        let plan = crate::buildplan::detect(&tmp.path().join("g"), None);
        assert_eq!(plan.kind, crate::buildplan::ProjectKind::CargoGame);
        assert!(plan.previewable);
        assert!(!crate::webserver::analyze(&tmp.path().join("g")).isolated);
    }
}
