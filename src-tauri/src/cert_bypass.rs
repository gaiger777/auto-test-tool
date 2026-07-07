//! macOS WKWebView 인증서 검증 우회.
//!
//! 내부 dev 서버들이 미신뢰 CA·호스트명 불일치·만료 등 제각각 깨진 TLS를 쓰기 때문에,
//! 캡처 창이 이런 사이트도 로드할 수 있도록 인증서 검증을 무시한다.
//! (curl -k / Postman "SSL 검증 끄기"와 동일. 내부 서버 대상 테스트 툴이라 허용하는 다운그레이드.)
//!
//! Tauri/wry의 WKWebView는 인증서 무시 옵션을 노출하지 않으므로, wry의 navigation delegate
//! 클래스에 objc 런타임으로 `webView:didReceiveAuthenticationChallenge:completionHandler:` 를 주입한다.
//! 모든 웹뷰가 같은 delegate 클래스를 공유하므로, 시작 시 메인 창 웹뷰의 delegate 클래스에
//! 1회 추가하면 이후 생성되는 캡처 창에도 자동 적용된다.

#[cfg(target_os = "macos")]
mod imp {
    use block2::Block;
    use objc2::runtime::{AnyClass, AnyObject, Sel};
    use objc2::{class, msg_send, sel};
    use std::ffi::c_void;

    // NSURLSessionAuthChallengeDisposition
    const USE_CREDENTIAL: isize = 0;
    const PERFORM_DEFAULT: isize = 1;

    /// wry delegate 클래스에 주입되는 인증 챌린지 핸들러.
    /// 서버 신뢰 챌린지면 무조건 신뢰 자격증명으로 수락 → 인증서 검증 무시.
    unsafe extern "C-unwind" fn accept_challenge(
        _this: *mut AnyObject,
        _cmd: Sel,
        _webview: *mut AnyObject,
        challenge: *mut AnyObject,
        completion: *mut Block<dyn Fn(isize, *mut AnyObject)>,
    ) {
        if challenge.is_null() || completion.is_null() {
            return;
        }
        let space: *mut AnyObject = msg_send![challenge, protectionSpace];
        if space.is_null() {
            (*completion).call((PERFORM_DEFAULT, std::ptr::null_mut()));
            return;
        }
        let trust: *mut c_void = msg_send![space, serverTrust];
        if trust.is_null() {
            // 서버 신뢰 챌린지가 아니면(예: 클라이언트 인증서) 기본 처리에 맡긴다
            (*completion).call((PERFORM_DEFAULT, std::ptr::null_mut()));
            return;
        }
        let cred: *mut AnyObject = msg_send![class!(NSURLCredential), credentialForTrust: trust];
        (*completion).call((USE_CREDENTIAL, cred));
    }

    pub unsafe fn install(webview_ptr: *mut c_void) {
        if webview_ptr.is_null() {
            return;
        }
        let webview = webview_ptr as *mut AnyObject;
        let delegate: *mut AnyObject = msg_send![webview, navigationDelegate];
        if delegate.is_null() {
            eprintln!("[cert_bypass] navigationDelegate 없음 — 인증서 우회 미적용");
            return;
        }
        let cls: *const AnyClass = msg_send![delegate, class];
        let selector = sel!(webView:didReceiveAuthenticationChallenge:completionHandler:);
        // 타입 인코딩: 반환 void, self(@), _cmd(:), webview(@), challenge(@), completion 블록(@?)
        // class_addMethod는 이미 메서드가 있으면 no-op(false 반환)이라 중복 가드가 필요 없다.
        let types = c"v@:@@@?";
        let imp_ptr: unsafe extern "C-unwind" fn() = std::mem::transmute(
            accept_challenge
                as unsafe extern "C-unwind" fn(
                    *mut AnyObject,
                    Sel,
                    *mut AnyObject,
                    *mut AnyObject,
                    *mut Block<dyn Fn(isize, *mut AnyObject)>,
                ),
        );
        objc2::ffi::class_addMethod(cls as *mut _, selector, imp_ptr, types.as_ptr());
        eprintln!("[cert_bypass] WKWebView 인증서 검증 우회 설치 완료");
    }
}

/// 주어진 WKWebView(포인터)의 delegate 클래스에 인증서 검증 우회를 설치한다.
#[cfg(target_os = "macos")]
pub fn install(webview_ptr: *mut std::ffi::c_void) {
    unsafe { imp::install(webview_ptr) }
}

#[cfg(not(target_os = "macos"))]
pub fn install(_webview_ptr: *mut std::ffi::c_void) {}
