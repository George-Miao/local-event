use std::{cell::Cell, future::Future, rc::Rc, time::Duration};

use compio::runtime::{Runtime, spawn};

trait GenericEvent: 'static {
    type Listener: Future<Output = ()>;

    fn new() -> Self;
    fn listen(&self) -> Self::Listener;
    fn notify_all(&self);
}

impl GenericEvent for local_event::Event {
    type Listener = local_event::EventListener;

    fn new() -> Self {
        Self::new()
    }

    fn listen(&self) -> Self::Listener {
        self.listen()
    }

    fn notify_all(&self) {
        self.notify(usize::MAX);
    }
}

impl GenericEvent for event_listener::Event {
    type Listener = event_listener::EventListener;

    fn new() -> Self {
        Self::new()
    }

    fn listen(&self) -> Self::Listener {
        self.listen()
    }

    fn notify_all(&self) {
        self.notify(usize::MAX);
    }
}

async fn wake_count<E: GenericEvent>() -> u64 {
    fn spawn_listener<E: GenericEvent>(event: &Rc<E>, counter: &Rc<Cell<u64>>) {
        let event = Rc::clone(event);
        let counter = Rc::clone(counter);

        spawn(async move {
            for _ in 0..1_000 {
                event.listen().await;
                counter.update(|count| count + 1);
            }
        })
        .detach();
    }

    let event = Rc::new(E::new());
    let counter = Rc::new(Cell::new(0));

    spawn_listener(&event, &counter);
    spawn_listener(&event, &counter);

    compio::time::sleep(Duration::from_millis(100)).await;
    event.notify_all();
    compio::time::sleep(Duration::from_millis(100)).await;

    counter.get()
}

#[test]
fn notification_wakes_each_listener_once() {
    Runtime::new().unwrap().block_on(async {
        let local = wake_count::<local_event::Event>().await;
        let upstream = wake_count::<event_listener::Event>().await;

        assert_eq!(local, upstream);
        assert_eq!(local, 2);
    });
}
