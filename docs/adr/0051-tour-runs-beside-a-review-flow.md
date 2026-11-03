# A Tour runs beside a review flow

A review flow holds a bucket lease from triage until its owned work drains (ADR 0044), and every ordinary action on that bucket is refused meanwhile. The Tour was refused too, which meant a Deep run waiting on cost confirmation blocked "Generate tour" with "A review flow is already using this review bucket", although nothing was running. The Tour is exempt from the lease, in both directions.

The lease exists because a flow pins one prepared diff (`diff-tmp`, `diff-annotated`) and every stage reads it by hash. The Tour was refused because it prepared its diff into the same two files, and a Tour whose diff hashes differently would rewrite them under a running reviewer. The TUI's Tour path does not apply the review-ignore filter a flow applies before hashing, so the two can differ. Nothing else was shared: the Tour writes only `tour.json`, which no flow stage reads.

So the Tour now prepares its diff in `tour-input/` inside the bucket, and the exemption follows from that. A running or queued Tour is not counted as a producer, so it neither stops a flow acquiring the bucket nor holds the lease open after the flow finishes. The Tour still takes an agent slot (ADR 0021), so it queues behind a full set of review agents rather than racing them.

The alternative was to release the lease while a Deep flow waits for confirmation. It was not taken because it fixes one state of one flow and leaves the Tour blocked for the whole of triage and the arena run.

## Consequences

- A Tour is generated against whatever diff is live when it starts, not the diff the flow pinned. The two can disagree after an edit; each carries its own diff hash, so neither is mistaken for the other.
- The Tour may read `review.json` from the bucket while a flow is rewriting it. The prompt only asks the agent to use it when its `diff_hash` matches, and nothing makes that read atomic.
- Any other action added later that writes only its own sidecar can take the same exemption, but it has to stop sharing the pinned diff files first.
