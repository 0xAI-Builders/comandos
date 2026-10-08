// Inert widget ancestry for exact GtkWorkspace helpers, inserted by t13_adapter_probe.py.
mod workspace_probe {
    use std::{cell::{Cell, RefCell}, collections::HashMap, ops::Deref, rc::Rc};
    mod gtk {
        use super::*;
        #[derive(Clone)] pub struct Widget(Rc<Node>);
        struct Node { parent: Option<Widget>, grabbed: Cell<bool> }
        impl PartialEq for Widget { fn eq(&self, other: &Self) -> bool { Rc::ptr_eq(&self.0, &other.0) } }
        impl Widget {
            pub fn new(parent: Option<&Widget>) -> Self { Self(Rc::new(Node { parent: parent.cloned(), grabbed: Cell::new(false) })) }
            pub fn parent(&self) -> Option<Widget> { self.0.parent.clone() }
            pub fn is_ancestor(&self, ancestor: &Widget) -> bool {
                let mut parent = self.parent();
                while let Some(node) = parent { if &node == ancestor { return true; } parent = node.parent(); }
                false
            }
            pub fn grab_focus(&self) { self.0.grabbed.set(true); }
            pub fn grabbed(&self) -> bool { self.0.grabbed.get() }
            pub fn style_context(&self) -> Style { Style }
        }
        pub struct Style;
        impl Style { pub fn has_class(&self, _: &str) -> bool { true } pub fn add_class(&self, _: &str) {} pub fn remove_class(&self, _: &str) {} }
        pub struct Notebook { node: Widget, pages: Vec<Widget>, current: Cell<u32> }
        impl Notebook {
            pub fn new(node: Widget, pages: Vec<Widget>) -> Self { Self { node, pages, current: Cell::new(0) } }
            pub fn clone(&self) -> Self { Self { node: self.node.clone(), pages: self.pages.clone(), current: Cell::new(self.current.get()) } }
            pub fn upcast<T>(&self) -> Widget { let _ = std::marker::PhantomData::<T>; self.node.clone() }
            pub fn page_num(&self, page: &Widget) -> Option<u32> { self.pages.iter().position(|p| p == page).map(|i| i as u32) }
            pub fn current_page(&self) -> Option<u32> { Some(self.current.get()) }
            pub fn set_current_page(&self, page: Option<u32>) { self.current.set(page.unwrap()); }
        }
        impl Deref for Notebook { type Target = Widget; fn deref(&self) -> &Widget { &self.node } }
    }
    struct GtkWorkspace { root: gtk::Notebook, pages: RefCell<HashMap<String, gtk::Widget>>, focus: RefCell<Option<String>> }
    impl GtkWorkspace {
        fn focused(&self) -> Option<String> { self.focus.borrow().clone() }
        // EXACT_WORKSPACE_METHODS
    }
    pub fn run() {
        let root = gtk::Widget::new(None);
        let group_a = gtk::Widget::new(Some(&root));
        let group_b = gtk::Widget::new(Some(&root));
        let orphan = gtk::Widget::new(Some(&root));
        let paned = gtk::Widget::new(Some(&group_b));
        let inner_paned = gtk::Widget::new(Some(&paned));
        let b = gtk::Widget::new(Some(&inner_paned));
        let c = gtk::Widget::new(Some(&inner_paned));
        let a = gtk::Widget::new(Some(&group_a));
        let detached = gtk::Widget::new(None);
        let ws = GtkWorkspace { root: gtk::Notebook::new(root, vec![group_a.clone(), group_b.clone(), orphan.clone()]), pages: RefCell::new([("a".into(), a), ("b".into(), b.clone()), ("c".into(), c.clone()), ("orphan".into(), orphan.clone()), ("detached".into(), detached)].into()), focus: RefCell::new(Some("c".into())) };
        assert_eq!(ws.page_index("c"), Some(1));
        assert_eq!(ws.page_index("a"), Some(0));
        assert_eq!(ws.page_index("orphan"), Some(2));
        assert_eq!(ws.page_index("detached"), None);
        assert_eq!(ws.page_index("missing"), None);
        assert_eq!(ws.page_key(&group_b).as_deref(), Some("b"));
        assert_eq!(ws.page_key(&orphan).as_deref(), Some("orphan"));
        ws.root.set_current_page(Some(1));
        ws.focus_page(&group_b);
        assert_eq!(ws.focused().as_deref(), Some("c"));
        assert!(c.grabbed());
        assert!(!b.grabbed());
        ws.root.set_current_page(Some(0));
        ws.focus_page(&group_a);
        assert_eq!(ws.focused().as_deref(), Some("a"));
        ws.root.set_current_page(Some(1));
        ws.focus_page(&group_b);
        assert_eq!(ws.focused().as_deref(), Some("b"));
        assert!(b.grabbed());
        assert_eq!(ws.root.current_page(), Some(1));
    }
}
