//! A tiny procedural window icon: a "j" in the Alacritty palette.

use eframe::egui::IconData;

pub fn window_icon() -> IconData {
    const N: usize = 64;
    let bg = [0x18, 0x18, 0x18, 0xff];
    let fg = [0x82, 0xb8, 0xc8, 0xff];
    let mut rgba = vec![0u8; N * N * 4];

    let set = |rgba: &mut Vec<u8>, x: usize, y: usize, c: [u8; 4]| {
        let i = (y * N + x) * 4;
        rgba[i..i + 4].copy_from_slice(&c);
    };
    // Rounded square background.
    let r = 12.0_f32;
    for y in 0..N {
        for x in 0..N {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let cx = fx.clamp(r, N as f32 - r);
            let cy = fy.clamp(r, N as f32 - r);
            if (fx - cx).hypot(fy - cy) <= r {
                set(&mut rgba, x, y, bg);
            }
        }
    }
    let mut rect = |x0: usize, y0: usize, x1: usize, y1: usize| {
        for y in y0..y1 {
            for x in x0..x1 {
                set(&mut rgba, x, y, fg);
            }
        }
    };
    rect(34, 12, 42, 20); // dot
    rect(34, 26, 42, 46); // stem
    rect(22, 46, 42, 54); // foot
    rect(22, 40, 30, 54); // hook

    IconData { rgba, width: N as u32, height: N as u32 }
}
