//! 멀티웹뷰 탭 창의 탭 레지스트리·레이아웃. 한 Window 안에 상단 탭바 웹뷰("*-tabs")와
//! 콘텐츠 웹뷰(사이트·콘솔)들을 자식으로 얹고, 활성 탭만 보이게 전환한다.
use serde::Serialize;

/// 상단 탭바 높이(논리 px).
pub const TAB_H: f64 = 40.0;

#[derive(Clone, Serialize)]
pub struct Tab {
    pub label: String,
    pub title: String,
    /// 주 사이트(캡처 세션) 탭은 닫을 수 없다(false). 콘솔 탭만 닫기 허용.
    pub closeable: bool,
}

/// 탭바 웹뷰로 브로드캐스트하는 창별 탭 상태.
#[derive(Clone, Serialize)]
pub struct TabsState {
    pub window: String,
    pub tabs: Vec<Tab>,
    pub active: String,
}

/// 창별 탭 목록·활성 탭·콘솔 라벨 카운터.
#[derive(Default)]
pub struct TabRegistry {
    pub tabs: Vec<Tab>,
    pub active: String,
    pub seq: u64,
}

impl TabRegistry {
    pub fn state(&self, window: &str) -> TabsState {
        TabsState {
            window: window.to_string(),
            tabs: self.tabs.clone(),
            active: self.active.clone(),
        }
    }
}

/// 창 크기에 맞춰 탭바(상단 전폭)와 콘텐츠 웹뷰(탭바 아래 전체)를 재배치한다.
pub fn relayout(window: &tauri::Window) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let phys = match window.inner_size() {
        Ok(s) => s,
        Err(_) => return,
    };
    let w = phys.width as f64 / scale;
    let h = phys.height as f64 / scale;
    for wv in window.webviews() {
        let (pos, size) = if wv.label().ends_with("-tabs") {
            (
                tauri::LogicalPosition::new(0.0, 0.0),
                tauri::LogicalSize::new(w, TAB_H),
            )
        } else {
            (
                tauri::LogicalPosition::new(0.0, TAB_H),
                tauri::LogicalSize::new(w, (h - TAB_H).max(0.0)),
            )
        };
        let _ = wv.set_position(pos);
        let _ = wv.set_size(size);
    }
}
