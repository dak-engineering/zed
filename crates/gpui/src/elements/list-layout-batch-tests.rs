use crate::{
    self as gpui, AppContext, Bounds, Context, IntoElement, ListAlignment, ListOffset,
    ListSizingBehavior, ListState, Render, Styled, TestAppContext, Window, canvas, list, point, px,
    size,
};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[derive(Default)]
struct Batches {
    epoch: usize,
    items: BTreeMap<usize, usize>,
    rendered: Vec<(usize, usize)>,
    painted: Vec<(usize, usize)>,
}

struct BatchView {
    state: ListState,
    batches: Rc<RefCell<Batches>>,
    infer: bool,
    autoscroll: bool,
}

impl Render for BatchView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let batches = self.batches.clone();
        let autoscroll = self.autoscroll;
        let element = list(self.state.clone(), move |index, _, _| {
            {
                let mut batches = batches.borrow_mut();
                assert!(batches.epoch > 0);
                let epoch = batches.epoch;
                batches.items.insert(index, epoch);
                batches.rendered.push((epoch, index));
            }
            let batches = batches.clone();
            canvas(
                move |bounds, window, _| {
                    let mut batches = batches.borrow_mut();
                    let epoch = batches.epoch;
                    assert_eq!(batches.items.get(&index), Some(&epoch));
                    batches.painted.push((epoch, index));
                    if autoscroll && index == 2 {
                        window.request_autoscroll(Bounds::from_corners(
                            point(bounds.left(), bounds.top() - px(30.)),
                            point(bounds.right(), bounds.top() + px(5.)),
                        ));
                    }
                },
                |_, _, _, _| {},
            )
            .w(px(100.))
            .h(px(20.))
            .into_any_element()
        });
        let batches = self.batches.clone();
        element
            .on_layout_batch(move |_, _| {
                let mut batches = batches.borrow_mut();
                batches.epoch += 1;
                batches.items.clear();
            })
            .with_sizing_behavior(if self.infer {
                ListSizingBehavior::Infer
            } else {
                ListSizingBehavior::Auto
            })
            .w(px(100.))
            .h(px(60.))
    }
}

#[gpui::test]
fn inferred_measurement_and_prepaint_have_separate_complete_batches(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let batches = Rc::new(RefCell::new(Batches::default()));
    batches.borrow_mut().items.insert(99, 0);
    cx.draw(point(px(0.), px(0.)), size(px(100.), px(60.)), |_, cx| {
        cx.new(|_| BatchView {
            state: ListState::new(2, ListAlignment::Top, px(60.)),
            batches: batches.clone(),
            infer: true,
            autoscroll: false,
        })
        .into_any_element()
    });
    let batches = batches.borrow();
    assert_eq!(batches.epoch, 2);
    assert_eq!(batches.rendered, [(1, 0), (1, 1), (2, 0), (2, 1)]);
    assert_eq!(batches.painted, [(2, 0), (2, 1)]);
    assert_eq!(batches.items, BTreeMap::from([(0, 2), (1, 2)]));
}

#[gpui::test]
fn autoscroll_retry_discards_first_batch_without_resetting_sibling_probes(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let batches = Rc::new(RefCell::new(Batches::default()));
    let state = ListState::new(5, ListAlignment::Top, px(10.)).measure_all();
    state.scroll_to(ListOffset {
        item_ix: 2,
        offset_in_item: px(0.),
    });
    cx.draw(point(px(0.), px(0.)), size(px(100.), px(60.)), |_, cx| {
        cx.new(|_| BatchView {
            state: state.clone(),
            batches: batches.clone(),
            infer: false,
            autoscroll: true,
        })
        .into_any_element()
    });
    assert_eq!(state.logical_scroll_top().item_ix, 0);
    assert_eq!(state.logical_scroll_top().offset_in_item, px(10.));
    let batches = batches.borrow();
    assert_eq!(batches.epoch, 2);
    assert_eq!(
        &batches.rendered[..5],
        [(1, 0), (1, 1), (1, 2), (1, 3), (1, 4)]
    );
    assert!(batches.rendered.contains(&(2, 0)));
    assert!(batches.rendered.contains(&(2, 3)));
    assert_eq!(
        batches.items,
        BTreeMap::from([(0, 2), (1, 2), (2, 2), (3, 2)])
    );
    assert!(batches.painted.contains(&(2, 0)));
}
