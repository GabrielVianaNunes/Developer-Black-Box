//! Renderização do ícone: um cubo preto brilhante com uma pequena luz AO LADO dele.
//!
//! O cubo é isométrico, com três faces (topo mais claro, esquerda média, direita mais escura),
//! reflexo especular no topo, faixa de brilho na lateral e arestas claras que dão contraste em
//! barras de tarefas escuras. Cubo e luz ficam inteiramente dentro do quadro, com margem, sem se
//! sobrepor. Cada pixel usa 4x4 amostras para suavizar as bordas em tamanhos pequenos.

use crate::Light;

const SQRT3: f32 = 1.732_050_8;

// Faces (do mais claro ao mais escuro): topo, esquerda, direita.
const TOP_LIGHT: [u8; 3] = [0x54, 0x58, 0x66];
const TOP_DARK: [u8; 3] = [0x26, 0x28, 0x30];
const LEFT_TOP: [u8; 3] = [0x25, 0x27, 0x2f];
const LEFT_BOTTOM: [u8; 3] = [0x11, 0x12, 0x17];
const RIGHT_TOP: [u8; 3] = [0x13, 0x14, 0x19];
const RIGHT_BOTTOM: [u8; 3] = [0x06, 0x07, 0x0a];
// Arestas.
const RIM_TOP: [u8; 3] = [0xf1, 0xf3, 0xf8];
const RIM_SIDE: [u8; 3] = [0xb9, 0xbd, 0xc9];
const CREASE: [u8; 3] = [0x7f, 0x84, 0x93];
const DOT_RING: [u8; 3] = [0x0b, 0x0b, 0x0f];

type P = (f32, f32);

/// Geometria em pixels de um ícone `size` x `size`.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub margin: f32,
    /// Caixa que envolve o cubo.
    pub box_left: f32,
    pub box_right: f32,
    pub box_top: f32,
    pub box_bottom: f32,
    pub cube_cx: f32,
    pub cube_cy: f32,
    /// Raio do hexágono (centro até um vértice).
    pub cube_r: f32,
    pub dot_cx: f32,
    pub dot_cy: f32,
    pub dot_r: f32,
}

pub fn layout(size: u32) -> Layout {
    let s = size as f32;
    let margin = (s / 16.0).round().max(1.0);
    let w = s - 2.0 * margin;
    // Em tamanhos pequenos a luz é proporcionalmente maior para continuar legível.
    let small = size <= 24;
    let cube_w = if small { 0.60 } else { 0.62 } * w;
    let r = (cube_w / SQRT3).min(w / 2.0);
    let half_w = r * SQRT3 / 2.0;
    let cy = s / 2.0;
    let dot_d = if small { 0.28 } else { 0.24 } * w;
    Layout {
        margin,
        box_left: margin,
        box_right: margin + 2.0 * half_w,
        box_top: cy - r,
        box_bottom: cy + r,
        cube_cx: margin + half_w,
        cube_cy: cy,
        cube_r: r,
        dot_cx: s - margin - dot_d / 2.0,
        dot_cy: cy,
        dot_r: dot_d / 2.0,
    }
}

/// Vértices do hexágono: topo, direita-cima, direita-baixo, base, esquerda-baixo, esquerda-cima.
fn hex(l: &Layout) -> [P; 6] {
    let (cx, cy, r) = (l.cube_cx, l.cube_cy, l.cube_r);
    let dx = r * SQRT3 / 2.0;
    [(cx, cy - r), (cx + dx, cy - r / 2.0), (cx + dx, cy + r / 2.0), (cx, cy + r), (cx - dx, cy + r / 2.0), (cx - dx, cy - r / 2.0)]
}

fn cross(a: P, b: P, p: P) -> f32 {
    (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
}

/// Ponto dentro de um polígono convexo (qualquer orientação).
fn inside(poly: &[P], p: P) -> bool {
    let mut pos = false;
    let mut neg = false;
    for i in 0..poly.len() {
        let c = cross(poly[i], poly[(i + 1) % poly.len()], p);
        pos |= c > 0.0;
        neg |= c < 0.0;
    }
    !(pos && neg)
}

fn seg_dist(a: P, b: P, p: P) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let len2 = abx * abx + aby * aby;
    let t = if len2 == 0.0 { 0.0 } else { (((p.0 - a.0) * abx + (p.1 - a.1) * aby) / len2).clamp(0.0, 1.0) };
    ((p.0 - (a.0 + t * abx)).powi(2) + (p.1 - (a.1 + t * aby)).powi(2)).sqrt()
}

fn lerp(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| (f32::from(a[i]) + (f32::from(b[i]) - f32::from(a[i])) * t).round() as u8)
}

fn toward_white(c: [u8; 3], a: f32) -> [u8; 3] {
    lerp(c, [255, 255, 255], a)
}

fn sample(l: &Layout, size: u32, x: f32, y: f32, light: Option<[u8; 3]>) -> Option<[u8; 3]> {
    let p = (x, y);

    // luz (borda escura fina + preenchimento), quando o ícone tem luz
    if let Some(light) = light {
        let d = ((x - l.dot_cx).powi(2) + (y - l.dot_cy).powi(2)).sqrt();
        let ring = (l.dot_r * 0.22).max(0.5);
        if d <= l.dot_r - ring {
            return Some(light);
        }
        if d <= l.dot_r {
            return Some(DOT_RING);
        }
    }

    // cubo
    let v = hex(l);
    if !inside(&v, p) {
        return None;
    }
    let c = (l.cube_cx, l.cube_cy);
    let r = l.cube_r;
    // Aresta mais fina em tamanhos pequenos, para não engolir as faces.
    let edge = (size as f32 / 22.0).max(0.72);

    // arestas externas: as duas de cima (aresta de luz) mais claras que as laterais
    let (mut best, mut best_i) = (f32::MAX, 0);
    for i in 0..6 {
        let dist = seg_dist(v[i], v[(i + 1) % 6], p);
        if dist < best {
            best = dist;
            best_i = i;
        }
    }
    if best < edge {
        return Some(if best_i == 0 || best_i == 5 { RIM_TOP } else { RIM_SIDE });
    }
    // vincos internos que separam as três faces (em 16 px o tom das faces já basta)
    let crease = edge * 0.6;
    if size >= 20 && (seg_dist(c, v[5], p) < crease || seg_dist(c, v[1], p) < crease || seg_dist(c, v[3], p) < crease) {
        return Some(CREASE);
    }

    if inside(&[v[0], v[1], c, v[5]], p) {
        // topo: degradê do canto de luz (esquerda-cima) para o oposto, com reflexo especular
        let t = ((x - l.box_left) / (l.box_right - l.box_left) * 0.6 + (y - l.box_top) / r * 0.4).clamp(0.0, 1.0);
        let mut col = lerp(TOP_LIGHT, TOP_DARK, t);
        if size <= 24 {
            col = toward_white(col, 0.22); // pouco espaço: o tom do topo é o que diz "cubo"
        }
        // faixa de brilho paralela à aresta superior esquerda
        let e = (v[5].0 - v[0].0, v[5].1 - v[0].1);
        let len = (e.0 * e.0 + e.1 * e.1).sqrt();
        let along = ((x - v[0].0) * e.0 + (y - v[0].1) * e.1) / (len * len);
        let n = (e.1 / len, -e.0 / len); // normal apontando para dentro do topo
        let depth = ((x - v[0].0) * n.0 + (y - v[0].1) * n.1).abs();
        if (0.14..0.66).contains(&along) && depth > edge * 0.9 + r * 0.06 && depth < edge * 0.9 + r * 0.20 {
            col = toward_white(col, 0.42);
        }
        return Some(col);
    }
    if inside(&[v[5], c, v[3], v[4]], p) {
        // lateral esquerda: degradê vertical e uma faixa de reflexo
        let t = (y - (l.cube_cy - r / 2.0)) / (1.5 * r);
        let mut col = lerp(LEFT_TOP, LEFT_BOTTOM, t);
        if size <= 24 {
            col = toward_white(col, 0.08);
        }
        let from_left = x - l.box_left;
        if from_left > r * 0.20 && from_left < r * 0.36 && y < l.cube_cy + r * 0.05 {
            col = toward_white(col, 0.16);
        }
        return Some(col);
    }
    // lateral direita: a mais escura
    let t = (y - (l.cube_cy - r / 2.0)) / (1.5 * r);
    Some(lerp(RIGHT_TOP, RIGHT_BOTTOM, t))
}

/// Pixels RGBA (linha a linha, de cima para baixo), `size * size * 4` bytes.
pub fn render(light: Light, size: u32) -> Vec<u8> {
    render_with(&layout(size), size, Some(light.rgb()))
}

/// Só o cubo, maior e centralizado, sem luz. É o ícone do executável: o Windows mostra o ícone do executável (ou do
/// atalho) no botão da barra de tarefas e não o troca por outro, então ele não pode carregar uma cor que vai ficar
/// errada. A cor do estado vai no selo (`render_dot`).
pub fn render_cube(size: u32) -> Vec<u8> {
    let mut l = layout(size);
    let s = size as f32;
    let w = s - 2.0 * l.margin;
    let r = (0.88 * w / SQRT3).min(w / 2.0);
    let half_w = r * SQRT3 / 2.0;
    l.cube_r = r;
    l.cube_cx = s / 2.0;
    l.box_left = l.cube_cx - half_w;
    l.box_right = l.cube_cx + half_w;
    l.box_top = l.cube_cy - r;
    l.box_bottom = l.cube_cy + r;
    render_with(&l, size, None)
}

fn render_with(l: &Layout, size: u32, lc: Option<[u8; 3]>) -> Vec<u8> {
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    const N: u32 = 4;
    for py in 0..size {
        for px in 0..size {
            let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
            for sy in 0..N {
                for sx in 0..N {
                    let x = px as f32 + (sx as f32 + 0.5) / N as f32;
                    let y = py as f32 + (sy as f32 + 0.5) / N as f32;
                    if let Some(c) = sample(l, size, x, y, lc) {
                        r += u32::from(c[0]);
                        g += u32::from(c[1]);
                        b += u32::from(c[2]);
                        n += 1;
                    }
                }
            }
            if n == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                out.extend_from_slice(&[(r / n) as u8, (g / n) as u8, (b / n) as u8, (n * 255 / (N * N)) as u8]);
            }
        }
    }
    out
}

/// Selo do botão da barra de tarefas: só a luz, pequena (um disco com borda escura fina), no meio de um quadro
/// transparente. O Windows o desenha sobre o canto do botão e o respeita mesmo com o ícone do executável fixo.
pub fn render_dot(light: Light, size: u32) -> Vec<u8> {
    let s = size as f32;
    let c = s / 2.0;
    let r = s * 0.24;
    let ring = (r * 0.28).max(0.6);
    let lc = light.rgb();
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    const N: u32 = 4;
    for py in 0..size {
        for px in 0..size {
            let (mut rr, mut gg, mut bb, mut n) = (0u32, 0u32, 0u32, 0u32);
            for sy in 0..N {
                for sx in 0..N {
                    let x = px as f32 + (sx as f32 + 0.5) / N as f32;
                    let y = py as f32 + (sy as f32 + 0.5) / N as f32;
                    let d = ((x - c).powi(2) + (y - c).powi(2)).sqrt();
                    let col = if d <= r - ring {
                        lc
                    } else if d <= r {
                        DOT_RING
                    } else {
                        continue;
                    };
                    rr += u32::from(col[0]);
                    gg += u32::from(col[1]);
                    bb += u32::from(col[2]);
                    n += 1;
                }
            }
            if n == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                out.extend_from_slice(&[(rr / n) as u8, (gg / n) as u8, (bb / n) as u8, (n * 255 / (N * N)) as u8]);
            }
        }
    }
    out
}

/// Monta um `.ico` com imagens BMP de 32 bits (uma por tamanho).
pub fn encode_ico(sizes: &[u32], light: Light) -> Vec<u8> {
    encode_ico_with(sizes, |s| render(light, s))
}

/// `.ico` só com o cubo (sem luz): o ícone do executável.
pub fn encode_ico_cube(sizes: &[u32]) -> Vec<u8> {
    encode_ico_with(sizes, render_cube)
}

fn encode_ico_with(sizes: &[u32], draw: impl Fn(u32) -> Vec<u8>) -> Vec<u8> {
    let images: Vec<(u32, Vec<u8>)> = sizes
        .iter()
        .map(|&s| {
            let rgba = draw(s);
            let mask_row = ((s + 31) / 32 * 4) as usize;
            let mut dib = Vec::new();
            dib.extend_from_slice(&40u32.to_le_bytes());
            dib.extend_from_slice(&s.to_le_bytes());
            dib.extend_from_slice(&(s * 2).to_le_bytes());
            dib.extend_from_slice(&1u16.to_le_bytes());
            dib.extend_from_slice(&32u16.to_le_bytes());
            dib.extend_from_slice(&[0u8; 24]);
            for row in (0..s).rev() {
                for col in 0..s {
                    let i = ((row * s + col) * 4) as usize;
                    dib.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
                }
            }
            dib.extend(std::iter::repeat(0u8).take(mask_row * s as usize));
            (s, dib)
        })
        .collect();

    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len();
    for (s, dib) in &images {
        let dim = if *s >= 256 { 0 } else { *s as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(dib.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += dib.len();
    }
    for (_, dib) in images {
        out.extend(dib);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{light_for, Light};
    use bb_core::RecorderState;

    const SIZES: [u32; 5] = [16, 24, 32, 48, 256];

    fn px(img: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }

    fn luma(p: [u8; 4]) -> f32 {
        0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2])
    }

    #[test]
    fn dot_center_shows_the_light_color_at_every_size() {
        for size in SIZES {
            for light in [Light::Green, Light::Red, Light::Gray] {
                let l = layout(size);
                let img = render(light, size);
                let p = px(&img, size, l.dot_cx as u32, l.dot_cy as u32);
                assert_eq!(&p[..3], &light.rgb(), "size {size} {light:?}");
                assert_eq!(p[3], 255);
            }
        }
    }

    #[test]
    fn recording_icon_is_green_and_every_other_state_is_not() {
        for size in SIZES {
            let l = layout(size);
            for st in [
                RecorderState::Recording,
                RecorderState::ManualPause,
                RecorderState::PrivacyBlocked,
                RecorderState::SafetyFault,
                RecorderState::Starting,
                RecorderState::ShuttingDown,
            ] {
                let img = render(light_for(st), size);
                let p = px(&img, size, l.dot_cx as u32, l.dot_cy as u32);
                assert_eq!(&p[..3] == Light::Green.rgb().as_slice(), st == RecorderState::Recording, "{st:?}");
            }
        }
    }

    #[test]
    fn light_is_beside_the_cube_never_over_it() {
        for size in SIZES {
            let l = layout(size);
            assert!(l.dot_cx - l.dot_r > l.box_right, "size {size}: light must start after the cube ends");
            let img = render(Light::Red, size);
            let red = Light::Red.rgb();
            for y in 0..size {
                for x in 0..size {
                    let p = px(&img, size, x, y);
                    if p[3] == 255 && p[..3] == red {
                        assert!(x as f32 >= l.box_right, "size {size}: light pixel at x={x} over the cube");
                    }
                }
            }
        }
    }

    #[test]
    fn everything_stays_inside_the_canvas_margin() {
        for size in SIZES {
            let l = layout(size);
            let m = l.margin as u32;
            let img = render(Light::Green, size);
            for y in 0..size {
                for x in 0..size {
                    if x < m || y < m || x >= size - m || y >= size - m {
                        assert_eq!(px(&img, size, x, y)[3], 0, "size {size}: opaque pixel in margin at ({x},{y})");
                    }
                }
            }
            assert!(l.dot_cx + l.dot_r <= (size - m) as f32 + 0.001);
            assert!(l.box_top >= l.margin - 0.001 && l.box_bottom <= size as f32 - l.margin + 0.001);
        }
    }

    #[test]
    fn green_and_red_icons_differ_only_around_the_light() {
        for size in SIZES {
            let l = layout(size);
            let g = render(Light::Green, size);
            let r = render(Light::Red, size);
            let mut differing = 0;
            for y in 0..size {
                for x in 0..size {
                    if px(&g, size, x, y) != px(&r, size, x, y) {
                        differing += 1;
                        assert!(x as f32 >= l.dot_cx - l.dot_r - 1.0, "size {size}: difference outside the light at ({x},{y})");
                    }
                }
            }
            assert!(differing > 0);
        }
    }

    // ---- o cubo ----

    #[test]
    fn the_cube_is_vertically_centered_and_has_no_stripe_or_colored_band() {
        for size in SIZES {
            let l = layout(size);
            assert!(((l.box_top + l.box_bottom) / 2.0 - size as f32 / 2.0).abs() < 0.01, "size {size}: not centered");
            let img = render(Light::Green, size);
            // todo pixel sólido do cubo é neutro (preto/cinza, no máximo levemente azulado): sem laranja
            for y in 0..size {
                for x in 0..size {
                    let p = px(&img, size, x, y);
                    if p[3] == 255 && (x as f32) < l.box_right {
                        let spread = p[..3].iter().max().unwrap() - p[..3].iter().min().unwrap();
                        assert!(spread <= 30, "size {size}: colored pixel {p:?} on the cube at ({x},{y})");
                    }
                }
            }
        }
    }

    #[test]
    fn the_three_faces_have_distinct_tones_light_top_mid_left_dark_right() {
        for size in [48u32, 256] {
            let l = layout(size);
            let img = render(Light::Green, size);
            let (cx, cy, r) = (l.cube_cx, l.cube_cy, l.cube_r);
            let top = luma(px(&img, size, cx as u32, (cy - r * 0.55) as u32));
            let left = luma(px(&img, size, (cx - r * 0.55) as u32, (cy + r * 0.25) as u32));
            let right = luma(px(&img, size, (cx + r * 0.55) as u32, (cy + r * 0.25) as u32));
            assert!(top > left + 4.0 && left > right + 4.0, "size {size}: top {top:.0} left {left:.0} right {right:.0}");
        }
    }

    #[test]
    fn the_cube_is_black_but_glossy() {
        let size = 256;
        let l = layout(size);
        let img = render(Light::Green, size);
        let (cx, cy, r) = (l.cube_cx, l.cube_cy, l.cube_r);
        // predominantemente preto: as faces laterais são escuras
        assert!(luma(px(&img, size, (cx + r * 0.5) as u32, (cy + r * 0.3) as u32)) < 40.0);
        assert!(luma(px(&img, size, (cx - r * 0.5) as u32, (cy + r * 0.5) as u32)) < 50.0);
        // brilho: no topo há um reflexo bem mais claro que o resto da face
        let mut top_lumas = Vec::new();
        for y in ((cy - r * 0.95) as u32)..((cy - r * 0.10) as u32) {
            let p = px(&img, size, cx as u32, y);
            if p[3] == 255 {
                top_lumas.push(luma(p));
            }
        }
        let (lo, hi) = top_lumas.iter().fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
        assert!(hi - lo > 40.0, "the top face needs a visible highlight (range {lo:.0}..{hi:.0})");
    }

    #[test]
    fn the_cube_has_a_light_edge_for_contrast_on_dark_taskbars() {
        for size in [24u32, 32, 48, 256] {
            let l = layout(size);
            let img = render(Light::Green, size);
            // a coluna da aresta esquerda (vertical) fica inteira dentro do contorno
            let edge = px(&img, size, l.box_left as u32, l.cube_cy as u32);
            assert!(luma(edge) > 120.0, "size {size}: edge pixel too dark: {edge:?}");
        }
    }

    #[test]
    fn the_cube_stays_legible_at_16_px() {
        let size = 16;
        let l = layout(size);
        let img = render(Light::Green, size);
        // há pixels claros (arestas) e escuros (faces) dentro do cubo
        let (mut lightest, mut darkest) = (0.0f32, 255.0f32);
        for y in 0..size {
            for x in 0..size {
                let p = px(&img, size, x, y);
                if p[3] == 255 && (x as f32) < l.box_right - 1.0 {
                    lightest = lightest.max(luma(p));
                    darkest = darkest.min(luma(p));
                }
            }
        }
        assert!(lightest > 150.0 && darkest < 40.0, "contrast too low at 16px: {darkest:.0}..{lightest:.0}");
    }

    #[test]
    fn ico_has_a_valid_directory_and_matching_offsets() {
        let ico = encode_ico(&[16, 32], Light::Red);
        assert_eq!(&ico[0..4], &[0, 0, 1, 0]);
        assert_eq!(u16::from_le_bytes([ico[4], ico[5]]), 2);
        let size0 = u32::from_le_bytes(ico[14..18].try_into().unwrap()) as usize;
        let off0 = u32::from_le_bytes(ico[18..22].try_into().unwrap()) as usize;
        let off1 = u32::from_le_bytes(ico[34..38].try_into().unwrap()) as usize;
        assert_eq!(off0, 6 + 32);
        assert_eq!(off1, off0 + size0);
        let total = ico.len();
        let size1 = u32::from_le_bytes(ico[30..34].try_into().unwrap()) as usize;
        assert_eq!(total, off1 + size1);
    }

    #[test]
    fn the_executable_icon_is_a_centered_cube_without_any_light() {
        for size in [16u32, 24, 32, 48, 256] {
            let img = render_cube(size);
            assert_eq!(img.len(), (size * size * 4) as usize);
            for p in img.chunks(4).filter(|p| p[3] == 255) {
                for light in [Light::Green, Light::Red, Light::Gray] {
                    assert_ne!([p[0], p[1], p[2]], light.rgb(), "size {size}: a light-colored pixel in the cube-only icon");
                }
            }
            // centralizado: a coluna mais à esquerda e a mais à direita com pixel visível ficam à mesma distância das bordas
            let col_has = |x: u32| (0..size).any(|y| px(&img, size, x, y)[3] > 0);
            let left = (0..size).find(|&x| col_has(x)).unwrap();
            let right = (0..size).rev().find(|&x| col_has(x)).unwrap();
            assert!((left as i32 - (size - 1 - right) as i32).abs() <= 1, "size {size}: left {left} right {right}");
            assert!(right - left + 1 >= size * 3 / 4, "size {size}: the cube should fill most of the width");
        }
    }

    #[test]
    fn the_executable_icon_file_has_a_valid_directory_for_the_cube_only_version() {
        let ico = encode_ico_cube(&[16, 32, 256]);
        assert_eq!(&ico[..4], &[0, 0, 1, 0]);
        assert_eq!(u16::from_le_bytes([ico[4], ico[5]]), 3);
        assert_ne!(ico, encode_ico(&[16, 32, 256], Light::Gray), "no light, so it differs from the lit one");
    }

    #[test]
    fn the_taskbar_dot_is_small_centered_and_has_the_light_color_in_the_middle() {
        for size in [16u32, 20, 24, 32, 40] {
            for light in [Light::Green, Light::Red, Light::Gray] {
                let img = render_dot(light, size);
                assert_eq!(img.len(), (size * size * 4) as usize);
                let mid = px(&img, size, size / 2, size / 2);
                assert_eq!([mid[0], mid[1], mid[2]], light.rgb(), "size {size}");
                assert_eq!(px(&img, size, 0, 0)[3], 0);
                assert_eq!(px(&img, size, size - 1, size - 1)[3], 0);
                let seen = img.chunks(4).filter(|p| p[3] > 0).count() as u32;
                assert!(seen * 100 >= size * size * 10 && seen * 100 <= size * size * 35, "size {size}: {seen} pixels, it must be small");
            }
        }
        assert_ne!(render_dot(Light::Green, 16), render_dot(Light::Red, 16));
        assert_ne!(render_dot(Light::Gray, 16), render_dot(Light::Red, 16));
    }
}
