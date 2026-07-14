//! Low-cost window thumbnail sampling via BitBlt + frame variance.

use super::rules::ScreenPresenceRules;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    RGBQUAD, SRCCOPY,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScreenVote {
    InGame,
    Menu,
    #[default]
    None,
}

#[derive(Debug, Default)]
pub struct ScreenSampleState {
    last_sample: Option<Instant>,
    last_variance: Option<f32>,
    last_vote: ScreenVote,
}

impl ScreenSampleState {
    pub fn sample(
        &mut self,
        hwnd: isize,
        rules: &ScreenPresenceRules,
        interval: Duration,
        now: Instant,
    ) -> ScreenVote {
        if self
            .last_sample
            .is_some_and(|at| now.duration_since(at) < interval)
        {
            return self.last_vote;
        }
        self.last_sample = Some(now);

        let width = rules.sample_w.clamp(16, 256);
        let height = rules.sample_h.clamp(16, 256);
        let vote = capture_variance(hwnd, width, height).map(|variance| {
            self.last_variance = Some(variance);
            if variance <= rules.variance_max_menu {
                ScreenVote::Menu
            } else if variance >= rules.variance_min_game {
                ScreenVote::InGame
            } else {
                ScreenVote::None
            }
        });
        self.last_vote = vote.unwrap_or(ScreenVote::None);
        self.last_vote
    }

    pub fn last_variance(&self) -> Option<f32> {
        self.last_variance
    }
}

fn capture_variance(hwnd: isize, width: u32, height: u32) -> Option<f32> {
    let hwnd = HWND(hwnd as *mut _);

    let screen_dc = unsafe { GetDC(Some(hwnd)) };
    if screen_dc.is_invalid() {
        return None;
    }
    let mem_dc = unsafe { CreateCompatibleDC(Some(screen_dc)) };
    if mem_dc.is_invalid() {
        unsafe {
            let _ = ReleaseDC(Some(hwnd), screen_dc);
        }
        return None;
    }
    let bmp = unsafe { CreateCompatibleBitmap(screen_dc, width as i32, height as i32) };
    if bmp.is_invalid() {
        unsafe {
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(Some(hwnd), screen_dc);
        }
        return None;
    }
    let old = unsafe { SelectObject(mem_dc, HGDIOBJ(bmp.0)) };
    let copied = unsafe {
        BitBlt(
            mem_dc,
            0,
            0,
            width as i32,
            height as i32,
            Some(screen_dc),
            0,
            0,
            SRCCOPY,
        )
        .is_ok()
    };
    if !copied {
        unsafe {
            let _ = SelectObject(mem_dc, old);
            let _ = DeleteObject(HGDIOBJ(bmp.0));
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(Some(hwnd), screen_dc);
        }
        return None;
    }

    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        bmiColors: [RGBQUAD::default(); 1],
    };
    let lines = unsafe {
        GetDIBits(
            mem_dc,
            bmp,
            0,
            height,
            Some(pixels.as_mut_ptr() as *mut _),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    unsafe {
        let _ = SelectObject(mem_dc, old);
        let _ = DeleteObject(HGDIOBJ(bmp.0));
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(Some(hwnd), screen_dc);
    }
    if lines == 0 {
        return None;
    }

    Some(pixel_variance(&pixels))
}

fn pixel_variance(pixels: &[u8]) -> f32 {
    if pixels.len() < 8 {
        return 0.0;
    }
    let mut sum: f64 = 0.0;
    let mut count = 0u64;
    for chunk in pixels.chunks_exact(4) {
        let lum =
            0.299 * f64::from(chunk[2]) + 0.587 * f64::from(chunk[1]) + 0.114 * f64::from(chunk[0]);
        sum += lum / 255.0;
        count += 1;
    }
    if count == 0 {
        return 0.0;
    }
    let mean = sum / count as f64;
    let mut var_sum = 0.0;
    for chunk in pixels.chunks_exact(4) {
        let lum =
            0.299 * f64::from(chunk[2]) + 0.587 * f64::from(chunk[1]) + 0.114 * f64::from(chunk[0]);
        let norm = lum / 255.0;
        let d = norm - mean;
        var_sum += d * d;
    }
    (var_sum / count as f64).sqrt() as f32
}
