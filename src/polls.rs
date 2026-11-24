//! Presentation snapshot of SDK-aggregated MSC3381 polls.
use matrix_sdk::ruma::{UserId, events::poll::start::PollKind};
use matrix_sdk_ui::timeline::{MsgLikeKind, TimelineItemContent};

#[derive(Clone, Debug)]
pub struct Poll {
    pub question: String,
    pub answers: Vec<Answer>,
    pub ended: bool,
    pub disclosed: bool,
    pub max_selections: u64,
}
#[derive(Clone, Debug)]
pub struct Answer {
    pub id: String,
    pub text: String,
    pub votes: usize,
    /// Matrix user IDs, exposed only when results are disclosed.
    pub voters: Vec<Voter>,
    pub selected: bool,
}
/// Keep identity separate from the room-specific presentation name.
#[derive(Clone, Debug)]
pub struct Voter {
    pub user_id: String,
    pub name: String,
}
impl Voter {
    pub fn new(user_id: String) -> Self {
        Self {
            name: user_id.clone(),
            user_id,
        }
    }
}

impl Poll {
    pub fn validate_vote(&self, answers: &[String]) -> Result<(), String> {
        let unique: std::collections::HashSet<_> = answers.iter().collect();
        if self.ended
            || answers.is_empty()
            || answers.len() as u64 > self.max_selections
            || unique.len() != answers.len()
            || answers
                .iter()
                .any(|id| !self.answers.iter().any(|a| &a.id == id))
        {
            return Err("Choose valid answers in an open poll".into());
        }
        Ok(())
    }

    pub fn from_content(content: &TimelineItemContent, own_user: &UserId) -> Option<Self> {
        let TimelineItemContent::MsgLike(content) = content else {
            return None;
        };
        let MsgLikeKind::Poll(poll) = &content.kind else {
            return None;
        };
        let results = poll.results();
        let ended = results.end_time.is_some();
        let disclosed = ended || results.kind == PollKind::Disclosed;
        Some(Self {
            question: results.question,
            answers: results
                .answers
                .into_iter()
                .map(|answer| {
                    let voters = results.votes.get(&answer.id);
                    let mut visible_voters = if disclosed {
                        voters.cloned().unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    visible_voters.sort_unstable();
                    Answer {
                        id: answer.id,
                        text: answer.text,
                        votes: if disclosed {
                            voters.map_or(0, Vec::len)
                        } else {
                            0
                        },
                        voters: visible_voters.into_iter().map(Voter::new).collect(),
                        selected: voters.is_some_and(|v| v.iter().any(|u| u == own_user.as_str())),
                    }
                })
                .collect(),
            ended,
            disclosed,
            max_selections: results.max_selections,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn poll() -> Poll {
        Poll {
            question: "Lunch?".into(),
            answers: ["soup", "salad"]
                .into_iter()
                .map(|id| Answer {
                    id: id.into(),
                    text: id.into(),
                    votes: 0,
                    voters: Vec::new(),
                    selected: false,
                })
                .collect(),
            ended: false,
            disclosed: true,
            max_selections: 1,
        }
    }
    #[test]
    fn votes_require_open_poll_known_distinct_answers_and_selection_limit() {
        let mut poll = poll();
        assert!(poll.validate_vote(&["soup".into()]).is_ok());
        for answers in [
            vec![],
            vec!["unknown".into()],
            vec!["soup".into(), "salad".into()],
            vec!["soup".into(), "soup".into()],
        ] {
            assert!(poll.validate_vote(&answers).is_err());
        }
        poll.max_selections = 2;
        assert!(poll.validate_vote(&["soup".into(), "salad".into()]).is_ok());
        poll.ended = true;
        assert!(poll.validate_vote(&["soup".into()]).is_err());
    }
}

/// Composer syntax: `/poll Question` followed by one answer per line.
pub fn compose(
    body: &str,
) -> Result<
    Option<matrix_sdk::ruma::events::poll::unstable_start::NewUnstablePollStartEventContent>,
    String,
> {
    use matrix_sdk::ruma::events::poll::{
        start::PollKind,
        unstable_start::{
            NewUnstablePollStartEventContent, UnstablePollAnswer, UnstablePollAnswers,
            UnstablePollStartContentBlock,
        },
    };
    let Some(input) = body
        .strip_prefix("/poll ")
        .or_else(|| body.strip_prefix("/poll\n"))
    else {
        return Ok(None);
    };
    let mut lines = input.lines();
    let question = lines.next().unwrap_or_default().trim();
    let answers: Vec<_> = lines.map(str::trim).collect();
    if question.is_empty()
        || !(2..=20).contains(&answers.len())
        || answers.iter().any(|a| a.is_empty())
    {
        return Err("Enter a question after /poll and 2–20 nonempty answers, one per line.".into());
    }
    let unique: std::collections::HashSet<_> = answers.iter().map(|a| a.to_lowercase()).collect();
    if unique.len() != answers.len() {
        return Err("Poll answers must be distinct.".into());
    }
    let options = UnstablePollAnswers::try_from(
        answers
            .iter()
            .enumerate()
            .map(|(i, a)| UnstablePollAnswer::new(i.to_string(), *a))
            .collect::<Vec<_>>(),
    )
    .map_err(|e| e.to_string())?;
    let mut poll = UnstablePollStartContentBlock::new(question, options);
    poll.kind = PollKind::Disclosed;
    Ok(Some(NewUnstablePollStartEventContent::plain_text(
        input, poll,
    )))
}

#[cfg(test)]
mod creation_tests {
    use super::*;
    #[test]
    fn creation_validates_question_answer_count_blanks_and_duplicates() {
        assert!(compose("ordinary message").unwrap().is_none());
        for input in [
            "/poll \nA\nB",
            "/poll Lunch?\nSoup",
            "/poll Lunch?\nSoup\n\nSalad",
            "/poll Lunch?\nSoup\nsoup",
        ] {
            assert!(compose(input).is_err(), "{input}");
        }
        let content = compose("/poll Lunch?\nSoup\nSalad").unwrap().unwrap();
        assert_eq!(content.poll_start.question.text, "Lunch?");
        assert_eq!(content.poll_start.answers.len(), 2);
        assert_eq!(content.poll_start.kind, PollKind::Disclosed);
        assert_ne!(
            content.poll_start.answers[0].id,
            content.poll_start.answers[1].id
        );
    }
}
