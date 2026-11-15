use iced::{
    Element, Subscription, Task,
    futures::SinkExt,
    widget::{button, column, text},
};

#[derive(Default)]
struct Probe {
    completed: u64,
    generation: u64,
    open: bool,
    deliveries: u64,
}
#[derive(Clone, Debug)]
enum Message {
    Run,
    Finished,
    ToggleView,
    Delivered(u64),
}
impl Probe {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ToggleView => {
                self.open = !self.open;
                self.generation += 1;
                Task::none()
            }
            Message::Delivered(generation) => {
                if self.open && generation == self.generation {
                    self.deliveries += 1;
                }
                Task::none()
            }
            Message::Run => Task::perform(
                async {
                    // Fails if Iced did not enter a Tokio runtime.
                    let _runtime = tokio::runtime::Handle::current();
                    tokio::task::yield_now().await;
                },
                |_| Message::Finished,
            ),
            Message::Finished => {
                self.completed += 1;
                tracing::debug!(target: "interconnect", "finite UI task completed");
                Task::none()
            }
        }
    }
    fn subscription(&self) -> Subscription<Message> {
        if self.open {
            Subscription::run_with(self.generation, view_events)
        } else {
            Subscription::none()
        }
    }
    fn view(&self) -> Element<'_, Message> {
        column![
            text("Milestone 0 runtime probe"),
            button("Run Tokio task").on_press(Message::Run),
            text(format!("Completed: {}", self.completed)),
            button(if self.open {
                "Close subscription"
            } else {
                "Open subscription"
            })
            .on_press(Message::ToggleView),
            text(format!("Subscription deliveries: {}", self.deliveries))
        ]
        .padding(24)
        .spacing(12)
        .into()
    }
}
fn view_events(generation: &u64) -> impl iced::futures::Stream<Item = Message> + use<> {
    let generation = *generation;
    iced::stream::channel(1, async move |mut output| {
        let (commands, input) = tokio::sync::mpsc::channel(1);
        let (events, mut received) = tokio::sync::mpsc::channel(1);
        // Iced drops this stream when the view closes. The guard cancels the
        // worker even if stream cancellation interrupts an await below.
        let worker = interconnect::ViewWorker::start(input, events);
        if commands.send(generation).await.is_ok() {
            while let Some(generation) = received.recv().await {
                if output.send(Message::Delivered(generation)).await.is_err() {
                    break;
                }
            }
        }
        let _ = worker.close().await;
    })
}
fn main() -> iced::Result {
    interconnect::init_tracing();
    iced::application(Probe::default, Probe::update, Probe::view)
        .subscription(Probe::subscription)
        .run()
}
