//! Node-only lifecycle proof of the shared Promise adapter.
#![cfg(target_arch = "wasm32")]
use comandos_web_dom::port;
use std::{
    cell::Cell,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::wasm_bindgen_test;

struct Probe {
    polls: Rc<Cell<u32>>,
    dropped: Rc<Cell<bool>>,
    outcome: Result<JsValue, JsValue>,
}
impl Future for Probe {
    type Output = Result<JsValue, JsValue>;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        self.polls.set(self.polls.get() + 1);
        Poll::Ready(self.outcome.clone())
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        self.dropped.set(true);
    }
}
#[wasm_bindgen_test]
async fn resolution_and_rejection_keep_identity_ticks_and_drop() {
    for reject in [false, true] {
        let value: JsValue = js_sys::Object::new().into();
        let polls = Rc::new(Cell::new(0));
        let dropped = Rc::new(Cell::new(false));
        let result = port::promise(Probe {
            polls: polls.clone(),
            dropped: dropped.clone(),
            outcome: if reject {
                Err(value.clone())
            } else {
                Ok(value.clone())
            },
        });
        assert_eq!(polls.get(), 0, "construction must not poll synchronously");
        assert!(!dropped.get());
        let result = JsFuture::from(result.unchecked_into::<js_sys::Promise>()).await;
        assert_eq!(result, if reject { Err(value) } else { Ok(value) });
        assert_eq!(polls.get(), 1);
        assert!(dropped.get(), "settled future must release captured state");
    }
}
#[wasm_bindgen_test]
async fn pending_work_only_settles_after_the_original_promise() {
    let mut resolve = None;
    let pending = js_sys::Promise::new(&mut |yes, _| resolve = Some(yes));
    let settled = Rc::new(Cell::new(false));
    let signal = settled.clone();
    let adapted = port::promise(async move {
        let value = JsFuture::from(pending).await?;
        signal.set(true);
        Ok(value)
    });
    JsFuture::from(js_sys::Promise::resolve(&JsValue::NULL))
        .await
        .unwrap();
    assert!(
        !settled.get(),
        "pending work must not be completed by adaptation"
    );
    let value: JsValue = js_sys::Object::new().into();
    resolve.unwrap().call1(&JsValue::UNDEFINED, &value).unwrap();
    assert_eq!(
        JsFuture::from(adapted.unchecked_into::<js_sys::Promise>()).await,
        Ok(value)
    );
    assert!(settled.get());
}
