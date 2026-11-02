# Operating Manual: how to run your AI company

## What these agents are (and aren't)
- Each agent is a **Claude Code subagent** defined in `.claude/agents/<name>.md`, with a role, an expertise persona, KPIs, a roadmap and rules.
- They **run when invoked** in a Claude Code session opened in this folder. They don't work in the background 24/7, and they don't remember past runs. Their memory is the files: `company/board.md`, the `company/roadmaps/*.md` files and `docs/`.
- Subagents can't call other subagents. The **main Claude session acts as the CEO's chief of staff**: it asks the CEO for priorities, then dispatches work to the right agents.
- They can't spend money, sign up for services, publish content or email people. They draft, and **you approve and press the button**.

## Weekly cadence (about 30 minutes of founder time)
1. **Monday planning:** "Ask the ceo agent to run weekly planning." The CEO reads the board and roadmaps, then picks the top 3 priorities and their owners.
2. **Execution:** "Have the backend-engineer do BE-1", or "Run the CTO team on this week's Now items."
3. **Friday update:** "Ask the ceo agent for the weekly founder update." You get progress, metrics, risks and the decisions you need to make.
4. You answer the **Decisions needed from founder** list. That's the job.

## Example prompts
- `Use the cpo agent to verify Stripe Marketplace competitors (CPO-1).`
- `Use the einvoice-standards-expert to verify all mandate dates (STD-1).`
- `Run backend-engineer on BE-1, then qa-engineer to validate it.`
- `Use the cfo and pricing-analyst to propose pricing.`

## Rules every agent follows
Evidence over opinion, with unverified claims marked. No legal or tax advice to customers. No spending or publishing without your approval. Each agent updates its roadmap and the board when done.
