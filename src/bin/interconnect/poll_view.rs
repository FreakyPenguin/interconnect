use super::{App, MAX_PENDING_OPERATIONS, Message, reaction_picker::Target};
use iced::{
    Element, Task,
    widget::{button, column, text},
};
use interconnect::{connected::MessageRow, polls::Poll};
#[derive(Clone, Debug)]
pub enum Event {
    Vote(Target, Vec<String>),
    Sent(Target, Result<(), String>),
}
impl App {
    pub(super) fn poll_view<'a>(
        &self,
        row: &MessageRow,
        poll: &'a Poll,
        thread: bool,
        generation: u64,
    ) -> Element<'a, Message> {
        let target = self.selection.room.as_ref().and_then(|(_, room)| {
            Some(Target {
                room: room.clone(),
                thread,
                generation,
                event: row.event_id.clone()?,
            })
        });
        let enabled = !poll.ended
            && !self.closing
            && !self.stopped
            && self.pending_ops < MAX_PENDING_OPERATIONS
            && target
                .as_ref()
                .is_some_and(|t| !self.poll_sends.contains(t))
            && self.demo.is_none();
        let mut content = column![text(&poll.question).size(16)].spacing(8);
        for answer in &poll.answers {
            let label = format!(
                "{}{}{}",
                if answer.selected { "✓ " } else { "" },
                answer.text,
                if poll.disclosed {
                    format!(" · {} votes", answer.votes)
                } else {
                    String::new()
                }
            );
            let mut answers: Vec<_> = if poll.max_selections > 1 {
                poll.answers
                    .iter()
                    .filter(|a| a.selected && a.id != answer.id)
                    .map(|a| a.id.clone())
                    .collect()
            } else {
                Vec::new()
            };
            if !answer.selected || poll.max_selections == 1 {
                answers.push(answer.id.clone());
            }
            let action = enabled
                .then(|| target.clone())
                .flatten()
                .filter(|_| poll.validate_vote(&answers).is_ok())
                .map(|t| Message::Poll(Event::Vote(t, answers)));
            content = content.push(button(text(label)).on_press_maybe(action));
        }
        content = content.push(
            text(if poll.ended {
                "Poll ended"
            } else if poll.disclosed {
                "Select an answer to vote"
            } else {
                "Results hidden until the poll ends"
            })
            .size(12),
        );
        if let Some((_, error)) = self
            .poll_error
            .as_ref()
            .filter(|(t, _)| Some(t) == target.as_ref())
        {
            content = content.push(text(error.clone()).size(12));
        }
        content.into()
    }
    pub(super) fn poll_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Vote(target, answers) => {
                if self.closing
                    || self.stopped
                    || self.pending_ops >= MAX_PENDING_OPERATIONS
                    || self.poll_sends.contains(&target)
                    || !self
                        .message_row(&target)
                        .and_then(|r| r.poll.as_ref())
                        .is_some_and(|p| p.validate_vote(&answers).is_ok())
                {
                    return Task::none();
                }
                let Some(composer) = self.reaction_composer(&target) else {
                    return Task::none();
                };
                self.poll_sends.insert(target.clone());
                self.poll_error = None;
                self.pending_ops += 1;
                Task::perform(
                    composer.vote_poll(target.event.clone(), answers),
                    move |result| Message::Poll(Event::Sent(target.clone(), result)),
                )
            }
            Event::Sent(target, result) => {
                if self.poll_sends.remove(&target) {
                    self.pending_ops -= 1;
                    if self.message_row(&target).is_some()
                        && let Err(error) = result
                    {
                        self.poll_error = Some((target, error));
                    }
                }
                Task::none()
            }
        }
    }
}
