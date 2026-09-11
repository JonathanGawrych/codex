use std::collections::VecDeque;

use crate::Prompt;
use crate::context_manager::estimate_item_token_count;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use codex_protocol::models::ResponseItem;
use codex_utils_output_truncation::approx_token_count;

/// Request-local compaction of imported message transcripts. No intermediate
/// result is installed in the session, so errors and cancellation leave its
/// context intact. Native tool histories use the ordinary compaction path:
/// their calls and outputs cannot be split at arbitrary message boundaries.
pub(crate) struct MessageCompactionBatches {
    pending: VecDeque<(ResponseItem, i64)>,
    input_budget: i64,
    maximum_input_budget: i64,
    in_flight_prefix_len: usize,
    in_flight_source_costs: Vec<i64>,
}

impl MessageCompactionBatches {
    pub(crate) fn prepare(
        prompt: &mut Prompt,
        context_window: Option<i64>,
    ) -> Result<Option<Self>> {
        let Some(context_window) = context_window else {
            return Ok(None);
        };
        if !prompt.input.iter().all(|item| {
            matches!(
                item,
                ResponseItem::Message { .. } | ResponseItem::Compaction { .. }
            )
        }) {
            return Ok(None);
        }
        let overhead = i64::try_from(approx_token_count(&prompt.base_instructions.text))
            .unwrap_or(i64::MAX)
            .saturating_add(
                i64::try_from(approx_token_count(&serde_json::to_string(&prompt.tools)?))
                    .unwrap_or(i64::MAX),
            );
        let costs: Vec<_> = prompt.input.iter().map(estimate_item_token_count).collect();
        if costs.iter().copied().fold(overhead, i64::saturating_add) <= context_window {
            return Ok(None);
        }
        // Reserve room for estimation error and the compaction response. Keep
        // every returned item, including retained user messages, in later requests.
        let input_budget = context_window.saturating_mul(3) / 4 - overhead;
        let mut batches = Self {
            pending: std::mem::take(&mut prompt.input)
                .into_iter()
                .zip(costs)
                .collect(),
            input_budget,
            maximum_input_budget: input_budget,
            in_flight_prefix_len: 0,
            in_flight_source_costs: Vec::new(),
        };
        prompt.input = batches.next_input(Vec::new())?;
        Ok(Some(batches))
    }

    pub(crate) fn has_remaining(&self) -> bool {
        !self.pending.is_empty()
    }

    pub(super) fn next_input(
        &mut self,
        mut compacted: Vec<ResponseItem>,
    ) -> Result<Vec<ResponseItem>> {
        let prefix_len = compacted.len();
        let mut remaining = self.input_budget.saturating_sub(
            compacted
                .iter()
                .map(estimate_item_token_count)
                .fold(0, i64::saturating_add),
        );
        let mut source_costs = Vec::new();
        while let Some((_, cost)) = self.pending.front() {
            if *cost > remaining {
                break;
            }
            let Some((item, cost)) = self.pending.pop_front() else {
                break;
            };
            remaining -= cost;
            compacted.push(item);
            source_costs.push(cost);
        }
        if source_costs.is_empty() {
            return Err(CodexErr::InvalidRequest(
                "Message history cannot fit another bounded compaction request. No context was replaced; all saved messages remain intact.".to_string(),
            ));
        }
        self.in_flight_prefix_len = prefix_len;
        self.in_flight_source_costs = source_costs;
        Ok(compacted)
    }

    /// Restores a rejected source slice and rebuilds it with half as many
    /// original messages. The service can count model input that is not
    /// visible to this client, so its context-window response is authoritative.
    pub(crate) fn retry_smaller(&mut self, input: &mut Vec<ResponseItem>) -> Result<bool> {
        if self.in_flight_source_costs.len() <= 1 {
            return Ok(false);
        }
        let expected_len = self
            .in_flight_prefix_len
            .saturating_add(self.in_flight_source_costs.len());
        if input.len() != expected_len {
            return Err(CodexErr::InvalidRequest(
                "Bounded compaction could not restore a rejected request. No context was replaced; all saved messages remain intact.".to_string(),
            ));
        }

        let mut prefix = std::mem::take(input);
        let source = prefix.split_off(self.in_flight_prefix_len);
        let source_with_costs: Vec<_> = source
            .into_iter()
            .zip(std::mem::take(&mut self.in_flight_source_costs))
            .collect();
        let retry_count = source_with_costs.len().div_ceil(2);
        let prefix_tokens = prefix
            .iter()
            .map(estimate_item_token_count)
            .fold(0, i64::saturating_add);
        let retry_tokens = source_with_costs
            .iter()
            .take(retry_count)
            .map(|(_, cost)| *cost)
            .fold(0, i64::saturating_add);
        for item in source_with_costs.into_iter().rev() {
            self.pending.push_front(item);
        }
        self.input_budget = prefix_tokens.saturating_add(retry_tokens);
        *input = self.next_input(prefix)?;
        Ok(true)
    }

    pub(crate) fn next_input_after_success(
        &mut self,
        compacted: Vec<ResponseItem>,
    ) -> Result<Vec<ResponseItem>> {
        self.input_budget = self.maximum_input_budget;
        self.next_input(compacted)
    }

    pub(crate) fn validate_output(&self, output: &[ResponseItem]) -> Result<()> {
        if !output
            .iter()
            .any(|item| matches!(item, ResponseItem::Compaction { .. }))
        {
            return Err(CodexErr::InvalidRequest(
                "Bounded compaction returned no summary. No context was replaced.".to_string(),
            ));
        }
        let output_tokens = output
            .iter()
            .map(estimate_item_token_count)
            .fold(0, i64::saturating_add);
        if output_tokens > self.maximum_input_budget {
            let input_budget = self.maximum_input_budget;
            return Err(CodexErr::InvalidRequest(format!(
                "Bounded compaction returned an estimated {output_tokens} tokens, exceeding its {input_budget}-token input budget. No context was replaced."
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "compact_message_batches_tests.rs"]
mod tests;
