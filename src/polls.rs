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

/// Editable creation state, shared by the form and the text-template path.
/// Incomplete values are allowed here so drafts can be saved before validation.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Creation {
    pub question: String,
    pub answers: Vec<String>,
    pub max_selections: u64,
    pub hidden_results: bool,
}
impl Default for Creation {
    fn default() -> Self {
        Self {
            question: String::new(),
            answers: vec![String::new(), String::new()],
            max_selections: 1,
            hidden_results: false,
        }
    }
}
impl Creation {
    pub fn to_content(
        &self,
    ) -> Result<
        matrix_sdk::ruma::events::poll::unstable_start::NewUnstablePollStartEventContent,
        String,
    > {
        use matrix_sdk::ruma::events::poll::unstable_start::{
            NewUnstablePollStartEventContent, UnstablePollAnswer, UnstablePollAnswers,
            UnstablePollStartContentBlock,
        };
        let question = self.question.trim();
        let answers: Vec<_> = self.answers.iter().map(|answer| answer.trim()).collect();
        if question.is_empty()
            || !(2..=20).contains(&answers.len())
            || answers.iter().any(|answer| answer.is_empty())
        {
            return Err("Enter a question and 2–20 nonempty answers.".into());
        }
        let unique: std::collections::HashSet<_> =
            answers.iter().map(|answer| answer.to_lowercase()).collect();
        if unique.len() != answers.len() {
            return Err("Poll answers must be distinct.".into());
        }
        if self.max_selections == 0 || self.max_selections > answers.len() as u64 {
            return Err("Choose a selection limit between 1 and the number of answers.".into());
        }
        let options = UnstablePollAnswers::try_from(
            answers
                .iter()
                .enumerate()
                .map(|(i, answer)| UnstablePollAnswer::new(i.to_string(), *answer))
                .collect::<Vec<_>>(),
        )
        .map_err(|error| error.to_string())?;
        let mut poll = UnstablePollStartContentBlock::new(question, options);
        poll.kind = if self.hidden_results {
            PollKind::Undisclosed
        } else {
            PollKind::Disclosed
        };
        poll.max_selections = self
            .max_selections
            .try_into()
            .map_err(|error| format!("{error}"))?;
        let fallback = std::iter::once(question)
            .chain(answers)
            .collect::<Vec<_>>()
            .join("\n");
        Ok(NewUnstablePollStartEventContent::plain_text(fallback, poll))
    }
}

/// Composer syntax: `/poll Question` followed by one answer per line.
/// This quick entry path retains disclosed, single-choice defaults.
pub fn compose(
    body: &str,
) -> Result<
    Option<matrix_sdk::ruma::events::poll::unstable_start::NewUnstablePollStartEventContent>,
    String,
> {
    let Some(input) = body
        .strip_prefix("/poll ")
        .or_else(|| body.strip_prefix("/poll\n"))
    else {
        return Ok(None);
    };
    let mut lines = input.lines();
    Creation {
        question: lines.next().unwrap_or_default().to_owned(),
        answers: lines.map(str::to_owned).collect(),
        ..Default::default()
    }
    .to_content()
    .map(Some)
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
        assert_eq!(u64::from(content.poll_start.max_selections), 1);
        assert_ne!(
            content.poll_start.answers[0].id,
            content.poll_start.answers[1].id
        );
    }
    #[test]
    fn creation_encodes_all_selection_and_disclosure_modes() {
        for hidden_results in [false, true] {
            for max_selections in [1, 2] {
                let draft = Creation {
                    question: " Lunch? ".into(),
                    answers: vec![" Soup ".into(), "Salad".into()],
                    max_selections,
                    hidden_results,
                };
                let content = draft.to_content().unwrap();
                assert_eq!(content.poll_start.question.text, "Lunch?");
                assert_eq!(content.poll_start.answers[0].text, "Soup");
                assert_eq!(u64::from(content.poll_start.max_selections), max_selections);
                assert_eq!(
                    content.poll_start.kind == PollKind::Undisclosed,
                    hidden_results
                );
                let json = serde_json::to_value(&content).unwrap();
                assert_eq!(
                    json["org.matrix.msc3381.poll.start"]["max_selections"],
                    max_selections
                );
            }
        }
    }

    #[test]
    fn creation_rejects_limits_and_answer_counts_outside_bounds() {
        let mut draft = Creation {
            question: "Lunch?".into(),
            answers: vec!["Soup".into(), "Salad".into()],
            ..Default::default()
        };
        for limit in [0, 3, u64::MAX] {
            draft.max_selections = limit;
            assert!(draft.to_content().is_err());
        }
        draft.max_selections = 1;
        draft.answers = (0..20).map(|i| format!("Answer {i}")).collect();
        assert!(draft.to_content().is_ok());
        draft.answers.push("One too many".into());
        assert!(draft.to_content().is_err());
        draft.answers = vec![" Soup ".into(), "soup".into()];
        assert!(draft.to_content().is_err());
    }
}
