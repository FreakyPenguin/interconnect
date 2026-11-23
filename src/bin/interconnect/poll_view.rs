use super::Message;
use iced::{
    Element,
    widget::{column, text},
};
use interconnect::polls::Poll;

pub fn view(poll: &Poll) -> Element<'_, Message> {
    let mut content = column![text(&poll.question).size(16)].spacing(8);
    for answer in &poll.answers {
        let label = if poll.disclosed {
            format!("{} · {} votes", answer.text, answer.votes)
        } else {
            answer.text.clone()
        };
        content = content.push(text(label));
    }
    content
        .push(
            text(if poll.ended {
                "Poll ended"
            } else if poll.disclosed {
                "Open poll"
            } else {
                "Results hidden until the poll ends"
            })
            .size(12),
        )
        .into()
}
