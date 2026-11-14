use accesskit::{
    ActionHandler, ActionRequest, ActivationHandler, Node, NodeId, Role, Tree, TreeId, TreeUpdate,
};
use accesskit_macos::Adapter;
use objc2::{
    msg_send,
    rc::{Id, autoreleasepool},
    runtime::{AnyObject, Bool},
    sel,
};
use objc2_app_kit::NSView;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSNumber, NSObject, NSPoint, NSRect, NSSize, NSString,
};

struct Actions;
impl ActionHandler for Actions {
    fn do_action(&mut self, _: ActionRequest) {
        panic!("native inspection must not dispatch actions");
    }
}

struct Initial;
impl ActivationHandler for Initial {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        let mut root = Node::new(Role::Window);
        root.set_children((2..=11).map(NodeId).collect::<Vec<_>>());
        let mut nodes = vec![(NodeId(1), root)];
        for level in 1..=6 {
            let mut heading = Node::new(Role::Heading);
            heading.set_label(format!("heading {level}"));
            heading.set_level(level);
            nodes.push((NodeId(level as u64 + 1), heading));
        }
        nodes.push((NodeId(8), Node::new(Role::Heading)));
        nodes.push((NodeId(9), Node::new(Role::Image)));
        nodes.push((NodeId(10), Node::new(Role::Link)));
        let mut braille = Node::new(Role::Label);
        braille.set_braille_label("dots");
        braille.set_braille_role_description("custom dots");
        nodes.push((NodeId(11), braille));
        Some(TreeUpdate {
            nodes,
            tree: Some(Tree::new(NodeId(1))),
            tree_id: TreeId::ROOT,
            focus: NodeId(1),
        })
    }
}

fn object_string(object: *mut AnyObject) -> String {
    assert!(!object.is_null(), "expected native string");
    // SAFETY: callers pass NSString-valued AppKit selectors within the autorelease pool.
    unsafe { (&*object.cast::<NSString>()).to_string() }
}

fn attribute(node: &NSObject, name: &str) -> *mut AnyObject {
    let name = NSString::from_str(name);
    // SAFETY: AccessKit PlatformNode implements the object-valued legacy attribute selector.
    unsafe { msg_send![node, accessibilityAttributeValue: &*name] }
}

fn child(children: &NSArray<NSObject>, index: usize) -> Id<NSObject> {
    assert!(index < children.count());
    // SAFETY: index is checked against this live NSArray's count before accessing its retained object.
    unsafe { children.objectAtIndex(index) }
}

fn verify(children: &NSArray<NSObject>) {
    assert_eq!(children.count(), 10);
    for index in 0..7 {
        let heading = child(children, index);
        // SAFETY: retained adapter nodes implement these documented AppKit selectors.
        unsafe {
            let role: *mut AnyObject = msg_send![&*heading, accessibilityRole];
            assert_eq!(object_string(role), "AXHeading", "heading index {index}");
            let value = attribute(&heading, "AXHeadingLevel");
            if index < 6 {
                assert!(!value.is_null(), "heading {} loses its level", index + 1);
                assert_eq!(
                    (&*value.cast::<NSNumber>()).unsignedIntegerValue(),
                    index + 1
                );
                let allowed: Bool = msg_send![&*heading, isAccessibilitySelectorAllowed: sel!(accessibilityAttributeValue:)];
                assert!(allowed.as_bool());
            } else {
                assert!(value.is_null(), "unset heading level must stay absent");
            }
        }
    }
    for (index, expected) in [(7, "AXImage"), (8, "AXLink")] {
        let node = child(children, index);
        // SAFETY: the retained adapter node implements NSString-valued accessibilityRole.
        let role: *mut AnyObject = unsafe { msg_send![&*node, accessibilityRole] };
        assert_eq!(object_string(role), expected);
        assert!(attribute(&node, "AXHeadingLevel").is_null());
    }
    let braille = child(children, 9);
    assert_eq!(object_string(attribute(&braille, "AXBrailleLabel")), "dots");
    assert_eq!(
        object_string(attribute(&braille, "AXBrailleRoleDescription")),
        "custom dots"
    );
    assert!(attribute(&braille, "AXHeadingLevel").is_null());
}

fn main() {
    autoreleasepool(|_| {
        let main_thread = MainThreadMarker::new().expect("AppKit needs the main thread");
        // SAFETY: NSView is allocated and initialized on the main thread and retained through adapter use.
        let view = unsafe {
            NSView::initWithFrame(
                main_thread.alloc(),
                NSRect::new(NSPoint::new(0., 0.), NSSize::new(200., 100.)),
            )
        };
        // SAFETY: view is a live NSView retained until after adapter and all native nodes are dropped.
        let mut adapter =
            unsafe { Adapter::new(Id::as_ptr(&view).cast_mut().cast(), false, Actions) };
        let children = adapter.view_children(&mut Initial);
        // SAFETY: public adapter returns an autoreleased NSArray valid throughout this pool.
        unsafe {
            let root = children.as_ref().expect("native root").objectAtIndex(0);
            let children: *mut NSArray<NSObject> = msg_send![&*root, accessibilityChildren];
            verify(children.as_ref().expect("semantic children"));
        }
        println!(
            "Native AXHeading levels 1-6, absent level, image/link and braille controls verified"
        );
    });
}
