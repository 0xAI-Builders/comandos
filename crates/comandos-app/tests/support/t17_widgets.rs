// Instrument only the GTK boundary. Real constructors are inserted unchanged.
use serde_json::{Value,json};
use std::{cell::RefCell,rc::Rc,io::Read};
#[derive(Clone)]
pub struct Node(Rc<RefCell<(String,Vec<String>,serde_json::Map<String,Value>,Vec<Node>)>>);
impl Node {
 fn new(kind:&str)->Self{Self(Rc::new(RefCell::new((kind.into(),vec![],serde_json::Map::new(),vec![]))))}
 fn property(&self,key:&str,value:impl Into<Value>){self.0.borrow_mut().2.insert(key.into(),value.into());}
 fn style_context(&self)->Node{self.clone()}
 fn add_class(&self,name:&str){self.0.borrow_mut().1.push(name.into());}
 fn set_markup(&self,text:&str){self.property("markup",text);}
 fn set_valign(&self,a:gtk::Align){self.property("valign",format!("{a:?}"));}
 fn set_halign(&self,a:gtk::Align){self.property("halign",format!("{a:?}"));}
 fn set_ellipsize(&self,_:pango::EllipsizeMode){self.property("ellipsize","End");}
 fn set_max_width_chars(&self,n:i32){self.property("max_width_chars",n);}
 fn set_tooltip_text(&self,text:Option<&str>){self.property("tooltip",text.unwrap_or_default());}
 fn set_relief(&self,_:gtk::ReliefStyle){self.property("relief","None");}
 fn pack_start(&self,child:&impl AsNode,_:bool,_:bool,_:u32){self.0.borrow_mut().3.push(child.node());}
 fn add(&self,child:&impl AsNode){self.0.borrow_mut().3.push(child.node());}
 fn add_provider(&self,p:&gtk::CssProvider,_:u32){let css=p.0.borrow();self.property("color",css.split("background-color:").nth(1).unwrap().split(';').next().unwrap());}
 fn json(&self)->Value{let data=self.0.borrow();json!({"kind":data.0,"classes":data.1,"properties":data.2,"children":data.3.iter().map(Node::json).collect::<Vec<_>>()})}
}
trait AsNode{fn node(&self)->Node;}
impl AsNode for Node{fn node(&self)->Node{self.clone()}}
macro_rules! widget {($name:ident)=>{#[derive(Clone)]pub struct $name(Node);impl std::ops::Deref for $name{type Target=Node;fn deref(&self)->&Node{&self.0}}impl AsNode for $name{fn node(&self)->Node{self.0.clone()}}};}
mod gtk {
 use super::*;
 #[derive(Debug)]pub enum Align{Start,Center}
 #[derive(Debug)]pub enum Orientation{Horizontal,Vertical}
 pub enum ReliefStyle{None}
 pub const STYLE_PROVIDER_PRIORITY_APPLICATION:u32=600;
 widget!(Box);widget!(Label);widget!(Separator);widget!(EventBox);widget!(Button);
 impl Box{pub fn new(o:Orientation,spacing:i32)->Self{let w=Self(Node::new("Box"));w.property("orientation",format!("{o:?}"));w.property("spacing",spacing);w}}
 impl Label{pub fn new(text:Option<&str>)->Self{let w=Self(Node::new("Label"));if let Some(text)=text{w.property("label",text);}w}}
 impl Separator{pub fn new(o:Orientation)->Self{let w=Self(Node::new("Separator"));w.property("orientation",format!("{o:?}"));w}}
 impl EventBox{pub fn new()->Self{Self(Node::new("EventBox"))}}
 impl Button{pub fn new()->Self{Self(Node::new("Button"))}}
 pub struct CssProvider(pub RefCell<String>);impl CssProvider{pub fn new()->Self{Self(RefCell::new(String::new()))}pub fn load_from_data(&self,b:&[u8])->Result<(),()>{*self.0.borrow_mut()=String::from_utf8(b.to_vec()).unwrap();Ok(())}}
}
mod pango{pub enum EllipsizeMode{End}}
fn svg_image(name:&str,size:i32,color:&str)->Node{let node=Node::new("SVG");node.property("icon",name);node.property("size",size);node.property("color",color);node}
type WebView=FakeView;
#[derive(Clone)]struct FakeView{stopped:Rc<std::cell::Cell<usize>>,unregistered:Rc<RefCell<Vec<String>>>}
struct FakeManager(Rc<RefCell<Vec<String>>>);
impl FakeView{fn stop_loading(&self){self.stopped.set(self.stopped.get()+1);}fn user_content_manager(&self)->Option<FakeManager>{Some(FakeManager(self.unregistered.clone()))}}
impl FakeManager{fn unregister_script_message_handler(&self,name:&str){self.0.borrow_mut().push(name.into());}}
use std::cell::Cell;
// ACTUAL_METHODS
fn main(){let mut raw=String::new();std::io::stdin().read_to_string(&mut raw).unwrap();let cases:Value=serde_json::from_str(&raw).unwrap();if cases.is_object(){
 let stopped=Rc::new(Cell::new(0));let unregistered=Rc::new(RefCell::new(vec![]));let closed=Rc::new(Cell::new(false));
 let fired=Rc::new(Cell::new(false));let f=fired.clone();let retry=Rc::new(RefCell::new(Some(glib::timeout_add_local_once(std::time::Duration::from_millis(1),move||f.set(true)))));
 let page=OwnedPage{view:FakeView{stopped:stopped.clone(),unregistered:unregistered.clone()},retry:retry.clone(),closed:closed.clone(),handler:"extensions".into()};page.cancel();page.cancel();drop(page);
 std::thread::sleep(std::time::Duration::from_millis(2));let context=glib::MainContext::default();while context.pending(){context.iteration(false);}
 println!("{}",json!({"closed":closed.get(),"retry_empty":retry.borrow().is_none(),"fired":fired.get(),"stops":stopped.get(),"unregisters":*unregistered.borrow()}));return;
 }let mut results=vec![];for item in cases.as_array().unwrap(){results.push(pane_card(&item["pane"],item["title"].as_str().unwrap(),item["fg"].as_str().unwrap()).json());}println!("{}",json!(results));}
