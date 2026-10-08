//! A job completion may open a modal loop while other jobs keep arriving.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[allow(dead_code)]
#[path = "../src/jobs.rs"]
mod jobs;

use std::{cell::Cell, rc::Rc, time::Duration};

#[test]
fn completion_can_run_a_nested_modal_loop_on_its_original_context() {
    let context = glib::MainContext::new();
    let _guard = context.acquire().unwrap();
    context
        .with_thread_default(|| {
            let outer = glib::MainLoop::new(Some(&context), false);
            let nested = glib::MainLoop::new(Some(&context), false);
            let delivered = Rc::new(Cell::new(0));
            let expected_thread = std::thread::current().id();
            let timeout = glib::timeout_source_new(
                Duration::from_secs(2),
                Some("job-modal-test-deadline"),
                glib::Priority::DEFAULT,
                {
                    let outer = outer.clone();
                    let nested = nested.clone();
                    move || {
                        nested.quit();
                        outer.quit();
                        glib::ControlFlow::Break
                    }
                },
            );
            timeout.attach(Some(&context));
            let first = jobs::to_main({
                let context = context.clone();
                let outer = outer.clone();
                let delivered = delivered.clone();
                move |value: usize| {
                    assert_eq!(std::thread::current().id(), expected_thread);
                    assert!(context.is_owner());
                    delivered.set(delivered.get() + value);
                    let second = jobs::to_main({
                        let delivered = delivered.clone();
                        let nested = nested.clone();
                        move |value: usize| {
                            assert_eq!(std::thread::current().id(), expected_thread);
                            delivered.set(delivered.get() + value);
                            nested.quit();
                        }
                    });
                    second.send_blocking(10).unwrap();
                    nested.run();
                    outer.quit();
                }
            });
            first.send_blocking(1).unwrap();
            outer.run();
            timeout.destroy();
            assert_eq!(
                delivered.get(),
                11,
                "both completions must be delivered once"
            );
        })
        .unwrap();
}
