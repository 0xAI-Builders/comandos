//! `OscLimiter` contra vte 0.15.0: lo que el filtro entrega, procesado por
//! vte, debe dar los mismos eventos que el flujo sin filtrar (salvo los OSC
//! que pasan del tope), y vte nunca debe despachar un OSC mayor que el tope.
use alacritty_terminal::vte::{Params, Parser, Perform};
use comandos_term::osc::{MAX_OSC_BYTES, OscLimiter};

/// Registra cada acción de vte como texto comparable.
#[derive(Default)]
struct Recorder {
    events: Vec<String>,
    /// Longitud cruda (parámetros + separadores) de cada OSC despachado.
    osc_lens: Vec<usize>,
}

impl Perform for Recorder {
    fn print(&mut self, c: char) {
        self.events.push(format!("print {c:?}"));
    }
    fn execute(&mut self, byte: u8) {
        self.events.push(format!("exec {byte:02x}"));
    }
    fn hook(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        self.events.push(format!(
            "hook {params:?} {intermediates:?} {ignore} {action:?}"
        ));
    }
    fn put(&mut self, byte: u8) {
        self.events.push(format!("put {byte:02x}"));
    }
    fn unhook(&mut self) {
        self.events.push("unhook".into());
    }
    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        let len = params.iter().map(|p| p.len()).sum::<usize>() + params.len().saturating_sub(1);
        self.osc_lens.push(len);
        self.events
            .push(format!("osc {params:?} {bell_terminated}"));
    }
    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        self.events.push(format!(
            "csi {params:?} {intermediates:?} {ignore} {action:?}"
        ));
    }
    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        self.events
            .push(format!("esc {intermediates:?} {ignore} {byte:02x}"));
    }
}

/// vte sin filtro, con los mismos trozos.
fn vte_plain(chunks: &[&[u8]]) -> Recorder {
    let (mut parser, mut rec) = (Parser::new(), Recorder::default());
    for chunk in chunks {
        parser.advance(&mut rec, chunk);
    }
    rec
}

/// vte detrás del filtro; devuelve también los bytes que el filtro entregó.
fn vte_filtered(limiter: &mut OscLimiter, chunks: &[&[u8]]) -> (Recorder, Vec<u8>) {
    let (mut parser, mut rec, mut fed) = (Parser::new(), Recorder::default(), Vec::new());
    for chunk in chunks {
        limiter.filter(chunk, |piece| {
            fed.extend_from_slice(piece);
            parser.advance(&mut rec, piece);
        });
    }
    (rec, fed)
}

fn filtered_bytes(limit: usize, chunks: &[&[u8]]) -> Vec<u8> {
    vte_filtered(&mut OscLimiter::with_limit(limit), chunks).1
}

/// Generador determinista (xorshift64*): sin dependencias nuevas.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Bytes que ejercitan todos los caminos de `Escape`, OSC, DCS, APC y UTF-8.
const ALPHABET: &[u8] = &[
    0x1B, b']', b']', 0x07, 0x18, 0x1A, b'a', b'\\', b';', b'0', b'[', b'P', b'_', b'X', b'^',
    b' ', b'(', 0x00, b'\n', 0x7F, 0x80, 0x9C, 0x9D, 0xC2, 0xE6, 0x98, 0xFF,
];

fn random_case(rng: &mut Rng) -> (Vec<u8>, Vec<usize>) {
    let len = rng.below(48);
    let data: Vec<u8> = (0..len)
        .map(|_| ALPHABET[rng.below(ALPHABET.len())])
        .collect();
    let mut cuts: Vec<usize> = (0..rng.below(7)).map(|_| rng.below(len + 1)).collect();
    cuts.sort_unstable();
    (data, cuts)
}

fn split<'a>(data: &'a [u8], cuts: &[usize]) -> Vec<&'a [u8]> {
    let mut chunks = Vec::new();
    let mut prev = 0;
    for &cut in cuts {
        chunks.push(&data[prev..cut]);
        prev = cut;
    }
    chunks.push(&data[prev..]);
    chunks
}

#[test]
fn differential_against_vte_with_random_streams_and_splits() {
    // Con el tope real ningún OSC del fuzz lo supera: los eventos deben ser
    // idénticos a vte sin filtro. Un OSC sin terminar al final queda retenido
    // en el filtro y a medias en vte; se cierran ambos con BEL para comparar.
    let mut rng = Rng(0x00C0_FFEE_D00D_F00D);
    for case in 0..60_000 {
        let (mut data, cuts) = random_case(&mut rng);
        data.push(0x07);
        let chunks = split(&data, &cuts);
        let plain = vte_plain(&chunks);
        let (filtered, _) = vte_filtered(&mut OscLimiter::default(), &chunks);
        assert_eq!(
            filtered.events, plain.events,
            "caso {case}: {data:02x?} cortes {cuts:?}"
        );
    }
}

#[test]
fn small_limit_bounds_every_osc_vte_dispatches_and_is_chunk_invariant() {
    let mut rng = Rng(0x5EED_0000_1234_5678);
    for case in 0..60_000 {
        let limit = 1 + rng.below(8);
        let (mut data, cuts) = random_case(&mut rng);
        data.push(0x07);
        let chunks = split(&data, &cuts);
        let (filtered, fed) = vte_filtered(&mut OscLimiter::with_limit(limit), &chunks);
        assert!(
            filtered.osc_lens.iter().all(|&len| len < limit),
            "caso {case}: tope {limit}, OSC {:?}, {data:02x?}",
            filtered.osc_lens
        );
        assert_eq!(
            fed,
            filtered_bytes(limit, &[&data]),
            "caso {case}: depende de los cortes"
        );
    }
}

#[test]
fn escape_survives_c0_del_and_high_bytes_before_the_bracket() {
    // vte sigue en `Escape` tras C0 (salvo CAN/SUB), DEL y 0x80–0xFF: el `]`
    // siguiente abre un OSC y el filtro tiene que retenerlo y acotarlo.
    for between in [
        0x00_u8, 0x05, b'\n', b'\r', 0x7F, 0x80, 0x9C, 0x9D, 0xC2, 0xFF, 0x1B,
    ] {
        let mut stream = vec![0x1B, between, b']'];
        stream.extend_from_slice(b"52;c;");
        stream.extend(std::iter::repeat_n(b'A', 64));
        stream.extend_from_slice(b"\x07ok");
        let (rec, fed) = vte_filtered(&mut OscLimiter::with_limit(16), &[&stream]);
        assert!(
            rec.osc_lens.is_empty(),
            "{between:02x}: se despachó {:?}",
            rec.osc_lens
        );
        assert_eq!(fed, [0x1B, between, 0x18, b'o', b'k'], "{between:02x}");
        assert!(
            rec.events
                .ends_with(&["print 'o'".into(), "print 'k'".into()])
        );
        // Con tope holgado el OSC llega entero.
        let (rec, _) = vte_filtered(&mut OscLimiter::default(), &[&stream]);
        assert_eq!(rec.osc_lens.len(), 1, "{between:02x}");
    }
}

#[test]
fn bytes_that_leave_escape_do_not_open_an_osc() {
    for between in [b' ', b'(', b'a', b'[', b'P', b'X', b'_', 0x18, 0x1A] {
        let stream = [0x1B, between, b']', b'x', 0x07];
        let mut l = OscLimiter::with_limit(1);
        assert_eq!(vte_filtered(&mut l, &[&stream]).1, stream, "{between:02x}");
        assert_eq!(l.buffered_len(), 0);
    }
}

#[test]
fn osc_split_at_every_byte_position_is_identical() {
    let stream: &[u8] =
        "a\x1b]0;título\x07b\x1b]52;c;aG9sYQ==\x1b\\c\x1b]8;;http://x\x1b\\d\x1b\n]2;t\x1a"
            .as_bytes();
    let whole = vte_plain(&[stream]).events;
    for cut in 0..=stream.len() {
        let (a, b) = stream.split_at(cut);
        let (rec, fed) = vte_filtered(&mut OscLimiter::default(), &[a, b]);
        assert_eq!(fed, stream, "corte {cut}");
        assert_eq!(rec.events, whole, "corte {cut}");
    }
    let bytes: Vec<&[u8]> = stream.chunks(1).collect();
    assert_eq!(
        vte_filtered(&mut OscLimiter::default(), &bytes).0.events,
        whole
    );
}

#[test]
fn c1_bytes_and_utf8_inside_and_outside_an_osc() {
    // Fuera de un OSC 0x9D no abre nada (vte lo ejecuta como C1) y no se retiene.
    let mut l = OscLimiter::with_limit(4);
    let outside: &[u8] = b"\x9d0;titulo-largo\x07\xc3\xa9\xe6\xbc\xa2";
    assert_eq!(vte_filtered(&mut l, &[outside]).1, outside);
    assert_eq!(l.buffered_len(), 0);
    // Dentro, 0x9C no termina el OSC y el UTF-8 viaja intacto.
    let inside: &[u8] = b"\x1b]0;\xc3\xa9\xe6\xbc\xa2\x9cx\x07";
    let (rec, fed) = vte_filtered(&mut OscLimiter::default(), &[&inside[..6], &inside[6..]]);
    assert_eq!(fed, inside);
    assert_eq!(rec.events, vte_plain(&[inside]).events);
    // UTF-8 partido justo antes del ESC que abre el OSC.
    let cut: &[u8] = b"\xe6\x1b]0;t\x07";
    let (rec, fed) = vte_filtered(&mut OscLimiter::default(), &[&cut[..1], &cut[1..]]);
    assert_eq!(fed, cut);
    assert_eq!(rec.events, vte_plain(&[&cut[..1], &cut[1..]]).events);
}

#[test]
fn can_and_sub_terminate_normal_and_oversized_oscs() {
    for end in [0x18_u8, 0x1A] {
        let normal = [b'\x1b', b']', b'0', b';', b't', end, b'z'];
        assert_eq!(filtered_bytes(MAX_OSC_BYTES, &[&normal]), normal);
        // Demasiado largo: se tira, CAN devuelve a vte a tierra y el CAN/SUB
        // final pasa (SUB llama a `substitute()`).
        let mut big = b"\x1b]0;".to_vec();
        big.extend(std::iter::repeat_n(b'A', 32));
        big.extend_from_slice(&[end, b'z']);
        let (rec, fed) = vte_filtered(&mut OscLimiter::with_limit(8), &[&big]);
        assert_eq!(fed, [0x1B, 0x18, end, b'z']);
        assert!(rec.osc_lens.is_empty());
        assert!(rec.events.contains(&format!("exec {end:02x}")));
    }
    // BEL solo cerraba el OSC largo: no suena.
    let mut big = b"\x1b]0;".to_vec();
    big.extend(std::iter::repeat_n(b'A', 32));
    big.push(0x07);
    let (rec, _) = vte_filtered(&mut OscLimiter::with_limit(8), &[&big]);
    assert!(!rec.events.contains(&"exec 07".to_string()));
}

#[test]
fn osc_inside_dcs_and_apc_strings() {
    // ESC dentro de DCS/APC devuelve a vte a `Escape`: el `]` abre un OSC.
    for prefix in [&b"\x1bP1$qm"[..], b"\x1b_payload", b"\x1bXsos", b"\x1b^pm"] {
        let mut stream = prefix.to_vec();
        stream.extend_from_slice(b"\x1b]0;");
        stream.extend(std::iter::repeat_n(b'A', 40));
        stream.extend_from_slice(b"\x07");
        let (rec, _) = vte_filtered(&mut OscLimiter::with_limit(8), &[&stream]);
        assert!(rec.osc_lens.is_empty(), "{prefix:?}");
        let (rec, fed) = vte_filtered(&mut OscLimiter::default(), &[&stream]);
        assert_eq!(fed, stream);
        assert_eq!(rec.events, vte_plain(&[&stream]).events);
    }
}

#[test]
fn several_oscs_in_one_chunk_and_one_right_after_an_esc_terminated_one() {
    let stream: &[u8] = b"\x1b]0;a\x1b]0;b\x07\x1b]2;c\x1b\\\x1b]52;c;aG9sYQ==\x07x";
    let mut l = OscLimiter::with_limit(16);
    let (rec, fed) = vte_filtered(&mut l, &[stream]);
    assert_eq!(fed, stream);
    assert_eq!(rec.osc_lens.len(), 4);
    assert_eq!(rec.events, vte_plain(&[stream]).events);
    assert_eq!(l.buffered_len(), 0);
}

#[test]
fn osc_held_by_the_filter_across_a_synchronized_update_and_tick() {
    use alacritty_terminal::index::{Column, Line, Point};
    use comandos_term::engine::{Engine, GridSize, Palette};
    let palette = Palette::xterm_default([255; 3], [0; 3], [255; 3], [0; 3], [64; 3]);
    let mut e = Engine::new(GridSize { cols: 20, rows: 3 }, 100, palette);
    let at = |e: &Engine, col: usize| e.term().grid()[Point::new(Line(0), Column(col))].c;
    // Empieza una actualización sincronizada y, dentro, un OSC a medias.
    e.advance(b"\x1b[?2026hA\x1b]0;ti", 0.0);
    assert_eq!(at(&e, 0), ' ');
    // Vence el plazo: vte vuelca lo suyo (A y el ESC); el OSC sigue en el filtro.
    assert!(e.tick(1_000.0));
    assert_eq!(at(&e, 0), 'A');
    assert_eq!(e.drain().title, None);
    // Llega el resto: el título se aplica entero y el texto sigue en orden.
    e.advance(b"tulo\x07B", 1_001.0);
    assert_eq!(e.drain().title.as_deref(), Some("titulo"));
    assert_eq!(at(&e, 1), 'B');
}
