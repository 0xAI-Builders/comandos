use comandos_term::{
    engine::{Modes, MouseMode},
    input::*,
};

fn k(key: &str) -> KeyInput<'_> {
    KeyInput {
        key,
        code: "",
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
    }
}
fn send(a: KeyAction) -> Vec<u8> {
    match a {
        KeyAction::Send(v) => v,
        other => panic!("{other:?}"),
    }
}

#[test]
fn xterm_js_key_table() {
    let n = Modes::default();
    let app = Modes {
        app_cursor: true,
        ..Modes::default()
    };
    let cases: &[(KeyInput, &Modes, &[u8])] = &[
        (k("ArrowUp"), &n, b"\x1b[A"),
        (k("ArrowUp"), &app, b"\x1bOA"),
        (
            KeyInput {
                shift: true,
                ..k("ArrowUp")
            },
            &n,
            b"\x1b[1;2A",
        ),
        (
            KeyInput {
                ctrl: true,
                ..k("ArrowRight")
            },
            &n,
            b"\x1b[1;5C",
        ),
        (k("Home"), &n, b"\x1b[H"),
        (k("Home"), &app, b"\x1bOH"),
        (k("End"), &n, b"\x1b[F"),
        (k("PageUp"), &n, b"\x1b[5~"),
        (k("Delete"), &n, b"\x1b[3~"),
        (k("Insert"), &n, b"\x1b[2~"),
        (k("F1"), &n, b"\x1bOP"),
        (k("F4"), &n, b"\x1bOS"),
        (k("F5"), &n, b"\x1b[15~"),
        (k("F12"), &n, b"\x1b[24~"),
        (k("Backspace"), &n, b"\x7f"),
        (
            KeyInput {
                ctrl: true,
                ..k("Backspace")
            },
            &n,
            b"\x08",
        ),
        (
            KeyInput {
                alt: true,
                ..k("Backspace")
            },
            &n,
            b"\x1b\x7f",
        ),
        (k("Tab"), &n, b"\t"),
        (
            KeyInput {
                shift: true,
                ..k("Tab")
            },
            &n,
            b"\x1b[Z",
        ),
        (k("Enter"), &n, b"\r"),
        (
            KeyInput {
                alt: true,
                ..k("Enter")
            },
            &n,
            b"\x1b\r",
        ),
        (k("Escape"), &n, b"\x1b"),
        (
            KeyInput {
                ctrl: true,
                ..k("c")
            },
            &n,
            b"\x03",
        ),
        (
            KeyInput {
                ctrl: true,
                ..k(" ")
            },
            &n,
            b"\x00",
        ),
        (
            KeyInput {
                ctrl: true,
                ..k("[")
            },
            &n,
            b"\x1b",
        ),
        (
            KeyInput {
                ctrl: true,
                ..k("\\")
            },
            &n,
            b"\x1c",
        ),
        (
            KeyInput {
                ctrl: true,
                ..k("]")
            },
            &n,
            b"\x1d",
        ),
        (
            KeyInput {
                alt: true,
                ..k("a")
            },
            &n,
            b"\x1ba",
        ),
        (k("ñ"), &n, "ñ".as_bytes()),
    ];
    for (key, modes, want) in cases {
        assert_eq!(send(encode_key(key, modes)), *want, "{}", key.key);
    }
}

#[test]
fn shift_pageup_scrolls_locally() {
    assert!(matches!(
        encode_key(
            &KeyInput {
                shift: true,
                ..k("PageUp")
            },
            &Modes::default()
        ),
        KeyAction::ScrollPage(-1)
    ));
}

#[test]
fn paste_normalises_newlines_and_brackets() {
    let br = Modes {
        bracketed_paste: true,
        ..Modes::default()
    };
    assert_eq!(encode_paste("a\nb\r\nc", &Modes::default()), b"a\rb\rc");
    assert_eq!(encode_paste("x\x1b[201~y", &br), b"\x1b[200~xy\x1b[201~");
}

#[test]
fn sgr_and_x10_mouse() {
    let sgr = Modes {
        mouse: MouseMode::Click,
        sgr_mouse: true,
        ..Modes::default()
    };
    let none = (false, false, false);
    assert_eq!(
        encode_mouse(Button::Left, MouseKind::Press, 53, 9, none, &sgr).unwrap(),
        b"\x1b[<0;54;10M"
    );
    assert_eq!(
        encode_mouse(Button::Left, MouseKind::Release, 53, 9, none, &sgr).unwrap(),
        b"\x1b[<0;54;10m"
    );
    assert!(
        encode_mouse(Button::Left, MouseKind::Move, 1, 1, none, &sgr).is_none(),
        "Click no informa movimiento"
    );
    let x10 = Modes {
        mouse: MouseMode::Click,
        ..Modes::default()
    };
    assert_eq!(
        encode_mouse(Button::Left, MouseKind::Press, 0, 0, none, &x10).unwrap(),
        b"\x1b[M !!"
    );
    assert!(encode_mouse(Button::Left, MouseKind::Press, 300, 0, none, &x10).is_none());
}

#[test]
fn wheel_policy_follows_the_spike() {
    let tmux_mouse = Modes {
        mouse: MouseMode::Drag,
        sgr_mouse: true,
        ..Modes::default()
    };
    assert!(matches!(wheel(-1, 0, 0, &tmux_mouse), WheelAction::Mouse(v) if v == b"\x1b[<64;1;1M"));
    let alt = Modes {
        alt_screen: true,
        alternate_scroll: true,
        ..Modes::default()
    };
    assert!(matches!(wheel(2, 0, 0, &alt), WheelAction::Arrows(v) if v == b"\x1b[B\x1b[B"));
    assert!(matches!(
        wheel(3, 0, 0, &Modes::default()),
        WheelAction::Scroll(3)
    ));
}

#[test]
fn gboard_beforeinput_bytes() {
    // Review Focus 4: los teclados en pantalla no mandan keydown.
    assert_eq!(beforeinput("deleteContentBackward", None).unwrap(), b"\x7f");
    assert_eq!(beforeinput("insertLineBreak", None).unwrap(), b"\r");
    assert_eq!(
        beforeinput("insertText", Some("ñandú")).unwrap(),
        "ñandú".as_bytes()
    );
    assert!(beforeinput("formatBold", None).is_none());
}

#[test]
fn ime_commit_once() {
    // Review Focus 4: Chrome manda compositionend y después input con el mismo texto.
    let mut ime = Ime::default();
    ime.start();
    assert!(ime.keydown_ignored(229));
    assert!(
        ime.input("insertCompositionText", Some("漢"), true)
            .is_none()
    );
    assert_eq!(ime.end("漢字").unwrap(), "漢字".as_bytes());
    assert!(
        ime.input("insertText", Some("漢字"), false).is_none(),
        "no se envía dos veces"
    );
    assert_eq!(ime.input("insertText", Some("a"), false).unwrap(), b"a");
}

// ---- Paridad fina con `Keyboard.evaluateKeyboardEvent` (Linux) ----

fn key_with<'a>(key: &'a str, code: &'a str, f: impl Fn(&mut KeyInput<'a>)) -> KeyInput<'a> {
    let mut v = KeyInput { code, ..k(key) };
    f(&mut v);
    v
}

#[test]
fn alt_arrows_become_ctrl_arrows_on_linux() {
    // Divergencia con el cuadro del encargo (ESC[1;3A): xterm.js en Linux
    // reescribe Alt+flecha a ESC[1;5X; el 1;3 es solo de macOS.
    let n = Modes::default();
    for (key, want) in [
        ("ArrowUp", &b"\x1b[1;5A"[..]),
        ("ArrowDown", b"\x1b[1;5B"),
        ("ArrowRight", b"\x1b[1;5C"),
        ("ArrowLeft", b"\x1b[1;5D"),
    ] {
        let alt = KeyInput {
            alt: true,
            ..k(key)
        };
        assert_eq!(send(encode_key(&alt, &n)), want, "{key}");
    }
    // Otras combinaciones conservan su parámetro.
    let sa = KeyInput {
        shift: true,
        alt: true,
        ..k("ArrowUp")
    };
    assert_eq!(send(encode_key(&sa, &n)), b"\x1b[1;4A");
    let ca = KeyInput {
        ctrl: true,
        alt: true,
        ..k("ArrowLeft")
    };
    assert_eq!(send(encode_key(&ca, &n)), b"\x1b[1;7D");
    // El modo de cursor de aplicación no cambia las formas con parámetro.
    let app = Modes {
        app_cursor: true,
        ..Modes::default()
    };
    let c = KeyInput {
        ctrl: true,
        ..k("ArrowUp")
    };
    assert_eq!(send(encode_key(&c, &app)), b"\x1b[1;5A");
}

#[test]
fn meta_arrows_send_nothing_but_meta_home_does() {
    let n = Modes::default();
    let m = KeyInput {
        meta: true,
        ..k("ArrowUp")
    };
    assert_eq!(encode_key(&m, &n), KeyAction::None);
    let h = KeyInput {
        meta: true,
        ..k("Home")
    };
    assert_eq!(send(encode_key(&h, &n)), b"\x1b[1;9H");
}

#[test]
fn modified_navigation_and_function_keys() {
    let n = Modes::default();
    let alt = |key| KeyInput {
        alt: true,
        ..k(key)
    };
    let ctrl = |key| KeyInput {
        ctrl: true,
        ..k(key)
    };
    let shift = |key| KeyInput {
        shift: true,
        ..k(key)
    };
    let cases: &[(KeyInput, &[u8])] = &[
        (alt("Home"), b"\x1b[1;3H"),
        (ctrl("End"), b"\x1b[1;5F"),
        (ctrl("Delete"), b"\x1b[3;5~"),
        (shift("Delete"), b"\x1b[3;2~"),
        (ctrl("PageUp"), b"\x1b[5;5~"),
        (ctrl("PageDown"), b"\x1b[6;5~"),
        // Alt+PageUp no lleva parámetro en xterm.js.
        (alt("PageUp"), b"\x1b[5~"),
        (k("PageDown"), b"\x1b[6~"),
        (alt("Insert"), b"\x1b[2~"),
        (shift("F5"), b"\x1b[15;2~"),
        (ctrl("F1"), b"\x1b[1;5P"),
        (k("F2"), b"\x1bOQ"),
        (k("F3"), b"\x1bOR"),
        (k("F6"), b"\x1b[17~"),
        (k("F7"), b"\x1b[18~"),
        (k("F8"), b"\x1b[19~"),
        (k("F9"), b"\x1b[20~"),
        (k("F10"), b"\x1b[21~"),
        (k("F11"), b"\x1b[23~"),
        (alt("F12"), b"\x1b[24;3~"),
        (ctrl("Tab"), b"\t"),
        (alt("Tab"), b"\t"),
        (shift("Backspace"), b"\x7f"),
        (alt("Escape"), b"\x1b\x1b"),
        (
            KeyInput {
                alt: true,
                ctrl: true,
                ..k("Backspace")
            },
            b"\x1b\x08",
        ),
    ];
    for (key, want) in cases {
        assert_eq!(send(encode_key(key, &n)), *want, "{key:?}");
    }
    assert_eq!(encode_key(&shift("Insert"), &n), KeyAction::None);
    assert_eq!(encode_key(&ctrl("Insert"), &n), KeyAction::None);
    assert_eq!(encode_key(&k("F13"), &n), KeyAction::None);
    assert!(matches!(
        encode_key(&shift("PageDown"), &n),
        KeyAction::ScrollPage(1)
    ));
    assert!(matches!(
        encode_key(
            &KeyInput {
                shift: true,
                ctrl: true,
                ..k("PageUp")
            },
            &n
        ),
        KeyAction::ScrollPage(-1)
    ));
}

#[test]
fn ctrl_combinations_follow_xterm_js() {
    let n = Modes::default();
    let ctrl = |key, code| key_with(key, code, |x| x.ctrl = true);
    assert_eq!(send(encode_key(&ctrl("3", "Digit3"), &n)), b"\x1b");
    assert_eq!(send(encode_key(&ctrl("4", "Digit4"), &n)), b"\x1c");
    assert_eq!(send(encode_key(&ctrl("5", "Digit5"), &n)), b"\x1d");
    assert_eq!(send(encode_key(&ctrl("6", "Digit6"), &n)), b"\x1e");
    assert_eq!(send(encode_key(&ctrl("7", "Digit7"), &n)), b"\x1f");
    assert_eq!(send(encode_key(&ctrl("8", "Digit8"), &n)), b"\x7f");
    // Sin tabla en xterm.js: Ctrl+/, Ctrl+- y Ctrl+2 no mandan nada.
    assert_eq!(encode_key(&ctrl("/", "Slash"), &n), KeyAction::None);
    assert_eq!(encode_key(&ctrl("-", "Minus"), &n), KeyAction::None);
    assert_eq!(encode_key(&ctrl("2", "Digit2"), &n), KeyAction::None);
    // Distribución no latina: la letra sale de `code`.
    assert_eq!(send(encode_key(&ctrl("с", "KeyC"), &n)), b"\x03");
    // Ctrl+Shift con `_` y `@` (únicos casos con Shift).
    let cs = |key| {
        key_with(key, "", |x| {
            x.ctrl = true;
            x.shift = true;
        })
    };
    assert_eq!(send(encode_key(&cs("_"), &n)), b"\x1f");
    assert_eq!(send(encode_key(&cs("@"), &n)), b"\x00");
    assert_eq!(encode_key(&cs("C"), &n), KeyAction::None);
    // Las teclas modificadoras solas no envían nada.
    assert_eq!(
        encode_key(&ctrl("Control", "ControlLeft"), &n),
        KeyAction::None
    );
}

#[test]
fn alt_characters_and_text() {
    let n = Modes::default();
    let alt = |key, code| key_with(key, code, |x| x.alt = true);
    assert_eq!(send(encode_key(&alt("1", "Digit1"), &n)), b"\x1b1");
    assert_eq!(
        send(encode_key(
            &key_with("!", "Digit1", |x| {
                x.alt = true;
                x.shift = true;
            }),
            &n
        )),
        b"\x1b!"
    );
    assert_eq!(send(encode_key(&alt(";", "Semicolon"), &n)), b"\x1b;");
    assert_eq!(send(encode_key(&alt(" ", "Space"), &n)), b"\x1b ");
    assert_eq!(
        send(encode_key(
            &key_with("A", "KeyA", |x| {
                x.alt = true;
                x.shift = true;
            }),
            &n
        )),
        b"\x1bA"
    );
    assert_eq!(
        send(encode_key(
            &key_with("a", "KeyA", |x| {
                x.alt = true;
                x.ctrl = true;
            }),
            &n
        )),
        b"\x1b\x01"
    );
    // Tecla muerta con Alt: ESC + letra de la tecla física.
    assert_eq!(send(encode_key(&alt("Dead", "KeyN"), &n)), b"\x1bn");
    assert_eq!(
        encode_key(&key_with("Dead", "KeyN", |_| {}), &n),
        KeyAction::None
    );
    // Texto.
    assert_eq!(send(encode_key(&k(" "), &n)), b" ");
    assert_eq!(
        send(encode_key(&key_with("A", "KeyA", |x| x.shift = true), &n)),
        b"A"
    );
    assert_eq!(send(encode_key(&k("€"), &n)), "€".as_bytes());
    // Un par sustituto llega por `input`, no por `keydown`.
    assert_eq!(encode_key(&k("😀"), &n), KeyAction::None);
    // Meta + carácter no escribe nada.
    assert_eq!(
        encode_key(&key_with("a", "KeyA", |x| x.meta = true), &n),
        KeyAction::None
    );
    assert_eq!(encode_key(&k("Unidentified"), &n), KeyAction::None);
    assert_eq!(encode_key(&k("Process"), &n), KeyAction::None);
}

#[test]
fn ios_arrow_keys() {
    let app = Modes {
        app_cursor: true,
        ..Modes::default()
    };
    assert_eq!(
        send(encode_key(&k("UIKeyInputUpArrow"), &Modes::default())),
        b"\x1b[A"
    );
    assert_eq!(send(encode_key(&k("UIKeyInputLeftArrow"), &app)), b"\x1bOD");
}

#[test]
fn paste_edge_cases() {
    let off = Modes::default();
    let br = Modes {
        bracketed_paste: true,
        ..Modes::default()
    };
    assert_eq!(encode_paste("a\r\r\nb\rc", &off), b"a\r\rb\rc");
    assert_eq!(encode_paste("", &off), b"");
    assert_eq!(encode_paste("", &br), b"\x1b[200~\x1b[201~");
    assert_eq!(encode_paste("ñ\n漢", &off), "ñ\r漢".as_bytes());
    assert_eq!(encode_paste("a\nb", &br), b"\x1b[200~a\rb\x1b[201~");
    // Quitar un cierre no debe fabricar otro con los bytes que lo rodean.
    assert_eq!(
        encode_paste("\x1b[20\x1b[201~1~", &br),
        b"\x1b[200~\x1b[201~"
    );
    assert_eq!(
        encode_paste("\x1b[2\x1b[2\x1b[201~01~01~", &br),
        b"\x1b[200~\x1b[201~"
    );
    // Sin corchetes no se toca nada más que los saltos de línea.
    assert_eq!(encode_paste("x\x1b[201~y", &off), b"x\x1b[201~y");
    // Con corchetes se quitan todas, también si el recorte une otra.
    assert_eq!(
        encode_paste("\x1b[201~\x1b[201~z\x1b[201", &br),
        b"\x1b[200~z\x1b[201\x1b[201~"
    );
}

const NOMODS: (bool, bool, bool) = (false, false, false);

#[test]
fn mouse_levels_and_buttons() {
    let drag = Modes {
        mouse: MouseMode::Drag,
        sgr_mouse: true,
        ..Modes::default()
    };
    let motion = Modes {
        mouse: MouseMode::Motion,
        ..drag
    };
    // Drag: mover con botón sí, sin botón no.
    assert_eq!(
        encode_mouse(Button::Left, MouseKind::Move, 0, 0, NOMODS, &drag).unwrap(),
        b"\x1b[<32;1;1M"
    );
    assert!(encode_mouse(Button::None, MouseKind::Move, 0, 0, NOMODS, &drag).is_none());
    // Motion: mover sin botón es el botón 3 con el bit de movimiento.
    assert_eq!(
        encode_mouse(Button::None, MouseKind::Move, 1, 2, NOMODS, &motion).unwrap(),
        b"\x1b[<35;2;3M"
    );
    // Sin botón solo existe el movimiento.
    assert!(encode_mouse(Button::None, MouseKind::Press, 0, 0, NOMODS, &motion).is_none());
    assert!(encode_mouse(Button::None, MouseKind::Release, 0, 0, NOMODS, &motion).is_none());
    // Botones y modificadores (ctrl 16, shift 4, alt 8).
    assert_eq!(
        encode_mouse(
            Button::Right,
            MouseKind::Press,
            0,
            0,
            (true, false, true),
            &drag
        )
        .unwrap(),
        b"\x1b[<22;1;1M"
    );
    assert_eq!(
        encode_mouse(
            Button::Middle,
            MouseKind::Release,
            0,
            0,
            (false, true, false),
            &drag
        )
        .unwrap(),
        b"\x1b[<9;1;1m"
    );
    // Ratón apagado.
    assert!(
        encode_mouse(
            Button::Left,
            MouseKind::Press,
            0,
            0,
            NOMODS,
            &Modes::default()
        )
        .is_none()
    );
    // Coordenadas grandes en SGR.
    assert_eq!(
        encode_mouse(Button::Left, MouseKind::Press, 1000, 2000, NOMODS, &drag).unwrap(),
        b"\x1b[<0;1001;2001M"
    );
}

#[test]
fn mouse_wheel_reports() {
    let sgr = Modes {
        mouse: MouseMode::Click,
        sgr_mouse: true,
        ..Modes::default()
    };
    assert_eq!(
        encode_mouse(Button::WheelUp, MouseKind::Press, 0, 0, NOMODS, &sgr).unwrap(),
        b"\x1b[<64;1;1M"
    );
    assert_eq!(
        encode_mouse(Button::WheelDown, MouseKind::Press, 2, 3, NOMODS, &sgr).unwrap(),
        b"\x1b[<65;3;4M"
    );
    assert!(encode_mouse(Button::WheelUp, MouseKind::Release, 0, 0, NOMODS, &sgr).is_none());
    assert!(encode_mouse(Button::WheelUp, MouseKind::Move, 0, 0, NOMODS, &sgr).is_none());
    let x10 = Modes {
        mouse: MouseMode::Click,
        ..Modes::default()
    };
    assert_eq!(
        encode_mouse(Button::WheelUp, MouseKind::Press, 0, 0, NOMODS, &x10).unwrap(),
        b"\x1b[M`!!"
    );
    assert_eq!(
        encode_mouse(Button::WheelDown, MouseKind::Press, 0, 0, NOMODS, &x10).unwrap(),
        b"\x1b[Ma!!"
    );
}

#[test]
fn x10_release_limits_and_raw_bytes() {
    let x10 = Modes {
        mouse: MouseMode::Click,
        ..Modes::default()
    };
    // En el formato clásico soltar es siempre el botón 3.
    assert_eq!(
        encode_mouse(Button::Right, MouseKind::Release, 0, 0, NOMODS, &x10).unwrap(),
        b"\x1b[M#!!"
    );
    assert_eq!(
        encode_mouse(Button::Middle, MouseKind::Press, 0, 0, NOMODS, &x10).unwrap(),
        b"\x1b[M!!!"
    );
    // La celda 222 (columna 223 en base 1) es la última representable.
    assert_eq!(
        encode_mouse(Button::Left, MouseKind::Press, 222, 0, NOMODS, &x10).unwrap(),
        [0x1b, b'[', b'M', 32, 255, 33]
    );
    assert!(encode_mouse(Button::Left, MouseKind::Press, 223, 0, NOMODS, &x10).is_none());
    assert!(encode_mouse(Button::Left, MouseKind::Press, 0, 223, NOMODS, &x10).is_none());
}

#[test]
fn wheel_details() {
    let tmux = Modes {
        mouse: MouseMode::Click,
        sgr_mouse: true,
        alt_screen: true,
        alternate_scroll: true,
        ..Modes::default()
    };
    // Un informe por rueda, sea cual sea la cantidad; con ratón no hay flechas.
    assert!(matches!(wheel(-3, 0, 0, &tmux), WheelAction::Mouse(v) if v == b"\x1b[<64;1;1M"));
    assert!(matches!(wheel(5, 1, 1, &tmux), WheelAction::Mouse(v) if v == b"\x1b[<65;2;2M"));
    let app_alt = Modes {
        alt_screen: true,
        alternate_scroll: true,
        app_cursor: true,
        ..Modes::default()
    };
    assert!(matches!(wheel(-2, 0, 0, &app_alt), WheelAction::Arrows(v) if v == b"\x1bOA\x1bOA"));
    // DECSET 1007 apagado: se desplaza la vista.
    let no_alt_scroll = Modes {
        alt_screen: true,
        ..Modes::default()
    };
    assert!(matches!(
        wheel(-4, 0, 0, &no_alt_scroll),
        WheelAction::Scroll(-4)
    ));
    assert!(matches!(wheel(0, 0, 0, &tmux), WheelAction::Scroll(0)));
    // X10 fuera de rango: ningún informe posible.
    let x10 = Modes {
        mouse: MouseMode::Click,
        ..Modes::default()
    };
    assert!(matches!(wheel(1, 300, 0, &x10), WheelAction::Scroll(0)));
}

#[test]
fn focus_reports() {
    let on = Modes {
        focus_events: true,
        ..Modes::default()
    };
    assert_eq!(focus(true, &on), Some(&b"\x1b[I"[..]));
    assert_eq!(focus(false, &on), Some(&b"\x1b[O"[..]));
    assert_eq!(focus(true, &Modes::default()), None);
}

#[test]
fn beforeinput_edge_cases() {
    assert_eq!(beforeinput("insertParagraph", None).unwrap(), b"\r");
    assert!(beforeinput("insertText", None).is_none());
    assert!(beforeinput("insertText", Some("")).is_none());
    assert!(beforeinput("insertCompositionText", Some("x")).is_none());
    assert!(beforeinput("insertFromPaste", Some("x")).is_none());
}

#[test]
fn ime_state_machine() {
    let mut ime = Ime::default();
    assert!(!ime.keydown_ignored(13));
    assert!(ime.keydown_ignored(229));
    // Sin composición, GBoard borra y escribe por `input`.
    assert_eq!(
        ime.input("deleteContentBackward", None, false).unwrap(),
        b"\x7f"
    );
    // Durante la composición no sale nada, ni el keydown.
    ime.start();
    assert!(ime.keydown_ignored(13));
    assert!(ime.input("insertText", Some("x"), false).is_none());
    assert!(
        ime.input("insertCompositionText", Some("x"), true)
            .is_none()
    );
    // Un compositionend vacío no envía ni deja eco.
    assert!(ime.end("").is_none());
    assert!(!ime.keydown_ignored(13));
    assert_eq!(ime.input("insertText", Some("x"), false).unwrap(), b"x");
    // El eco solo se traga una vez.
    ime.start();
    assert_eq!(ime.end("ab").unwrap(), b"ab");
    assert!(ime.input("insertText", Some("ab"), false).is_none());
    assert_eq!(ime.input("insertText", Some("ab"), false).unwrap(), b"ab");
    // Otro `input` de por medio cancela el eco.
    ime.start();
    ime.end("q");
    assert_eq!(ime.input("insertLineBreak", None, false).unwrap(), b"\r");
    assert_eq!(ime.input("insertText", Some("q"), false).unwrap(), b"q");
    // Una tecla enviada por keydown también lo cancela.
    ime.start();
    ime.end("z");
    ime.forget_commit();
    assert_eq!(ime.input("insertText", Some("z"), false).unwrap(), b"z");
}
