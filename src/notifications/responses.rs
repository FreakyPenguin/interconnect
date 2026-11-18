//! Response registrations are removed on completion, cancellation, or eviction.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::oneshot;

#[derive(Default)]
pub(super) struct Responses(Mutex<HashMap<String, oneshot::Sender<bool>>>);

impl Responses {
    pub(super) fn register(
        self: &Arc<Self>,
        id: String,
    ) -> (Registration, oneshot::Receiver<bool>) {
        let (sender, receiver) = oneshot::channel();
        self.0.lock().unwrap().insert(id.clone(), sender);
        (
            Registration {
                responses: self.clone(),
                id,
            },
            receiver,
        )
    }

    pub(super) fn complete(&self, id: &str, clicked: bool) {
        let sender = self.0.lock().unwrap().remove(id);
        if let Some(sender) = sender {
            let _ = sender.send(clicked);
        }
    }
}

pub(super) struct Registration {
    responses: Arc<Responses>,
    id: String,
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.responses.0.lock().unwrap().remove(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn response_and_dismissal_complete_once_and_unknown_ids_are_ignored() {
        let responses = Arc::new(Responses::default());
        let (_registration, receiver) = responses.register("click".into());
        responses.complete("other", true);
        responses.complete("click", true);
        responses.complete("click", false);
        assert!(receiver.await.unwrap());
        let (_registration, receiver) = responses.register("dismiss".into());
        responses.complete("dismiss", false);
        assert!(!receiver.await.unwrap());
        assert!(responses.0.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn dropping_unpolled_or_pending_waiters_removes_native_registrations() {
        let responses = Arc::new(Responses::default());
        for poll in [false, true] {
            let (registration, receiver) = responses.register("pending".into());
            let mut response = Box::pin(async move {
                let _registration = registration;
                receiver.await.unwrap_or(false)
            });
            if poll {
                assert!(iced::futures::poll!(&mut response).is_pending());
            }
            assert_eq!(responses.0.lock().unwrap().len(), 1);
            drop(response);
            assert!(responses.0.lock().unwrap().is_empty());
            responses.complete("pending", true);
        }
    }
}
