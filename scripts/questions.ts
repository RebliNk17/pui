import { Type } from "typebox";

export const QUESTIONS_TITLE = "Pui questions";
const text = (maxLength: number) => Type.String({ minLength: 1, maxLength });
const parameters = Type.Object({
  questions: Type.Array(Type.Object({
    id: text(80),
    question: text(2000),
    multiSelect: Type.Optional(Type.Boolean({ description: "Allow selecting several options. Pui waits for Next/Submit instead of advancing on click. Defaults to false." })),
    options: Type.Array(Type.Object({
      label: text(160),
      description: Type.Optional(Type.String({ maxLength: 1000 })),
    }), { maxItems: 8, description: "Choices, with optional explanations shown below each label. Use [] for a free-text question. Something else and Skip are supplied by Pui. Set multiSelect=true when more than one choice is allowed." }),
  }), { minItems: 1, maxItems: 8 }),
});

export function installQuestions(pi: any) {
  pi.registerTool({
    name: "pui_ask_questions",
    label: "Ask questions",
    exposure: "model-only",
    executionMode: "sequential",
    description: "Ask the user one or more clarification questions in Pui's native question flow. Each question offers single-choice or multi-select options (multiSelect=true), optional descriptions, Something else, and Skip. Waits for all answers, then the agent automatically continues. Not a permission grant. Do not use for tool approvals.",
    promptSnippet: "Ask one or more questions with choices, explanations, or a custom answer",
    promptGuidelines: ["Use pui_ask_questions instead of a prose list when you need the user's decisions before proceeding. Batch related questions in one call, put a recommended choice first and explain tradeoffs in descriptions. Do not ask unnecessary questions. Skipped questions and cancellation are not approval."],
    parameters,
    async execute(_id: string, args: any, signal: AbortSignal | undefined, _update: any, ctx: any) {
      if (!ctx.hasUI || ctx.mode !== "rpc") throw new Error("Pui questions require the native Pui app.");
      const questions = args.questions;
      if (!Array.isArray(questions) || !questions.length || questions.length > 8 || new Set(questions.map((q: any) => q.id)).size !== questions.length) throw new Error("Questions must have unique IDs (1–8 questions).");
      const request_id = crypto.randomUUID();
      const cancelled = () => ({ content: [{ type: "text", text: "The user cancelled the questions. No answers or approval were given. Wait for the user's next instruction; do not ask again automatically." }], details: { questions, answers: [], cancelled: true }, terminate: true });
      try {
        signal?.throwIfAborted();
        // Supported RPC transport, like the agent-control bridge. No stdout writes,
        // provider calls, timeout, injected user prompt, or extra agent loop.
        const value = await ctx.ui.input(QUESTIONS_TITLE, JSON.stringify({ version: 1, request_id, questions }), { signal });
        signal?.throwIfAborted();
        if (value === undefined) return cancelled();
        const result = JSON.parse(value);
        if (result.cancelled === true) return cancelled();
        if (!Array.isArray(result.answers) || result.answers.length !== questions.length) throw new Error("Incomplete question answers");
        const answers = result.answers.map((answer: any, i: number) => {
          const q = questions[i];
          if (answer.id !== q.id) throw new Error("Mismatched question answer");
          if (answer.skipped === true) return { id: q.id, question: q.question, skipped: true };
          const custom = answer.custom === true;
          if (q.multiSelect === true) {
            const selections = answer.selections;
            if (!Array.isArray(selections) || selections.length > q.options.length || new Set(selections).size !== selections.length || selections.some((s: any) => !q.options.some((o: any) => o.label === s))) throw new Error("Invalid multi-select choices");
            if (!selections.length && !custom) throw new Error("Empty multi-select answer; use Skip instead");
            if (custom && (typeof answer.answer !== "string" || !answer.answer.trim() || answer.answer.length > 4000)) throw new Error("Invalid custom answer");
            return { id: q.id, question: q.question, selections, ...(custom ? { answer: answer.answer } : {}), custom, skipped: false };
          }
          if (typeof answer.answer !== "string" || !answer.answer.trim() || answer.answer.length > 4000) throw new Error("Invalid question answer");
          if (!custom && !q.options.some((o: any) => o.label === answer.answer)) throw new Error("Unknown question choice");
          return { id: q.id, question: q.question, answer: answer.answer, custom, skipped: false };
        });
        const details = { questions, answers, cancelled: false };
        let summary = JSON.stringify({ answers, note: "Continue using these answers. Skipped questions are unanswered, not permission to perform restricted actions." });
        if (Buffer.byteLength(summary, "utf8") > 48000) {
          // Keep structured details intact; bound only model-facing text.
          summary = JSON.stringify({ answers: answers.map((a: any) => ({ ...a, question: undefined, ...(a.answer ? { answer: Array.from(a.answer).slice(0, 500).join("") } : {}) })), note: "Long custom answers were truncated to 500 characters. Full answers are in tool details. Skipped questions are not approval." });
        }
        return { content: [{ type: "text", text: summary }], details };
      } finally {
        // RPC input cancellation has no dismissal event of its own. Remove only
        // this request, including when Stop is pressed while other tools run.
        ctx.ui.setStatus("pui-questions", JSON.stringify({ closed: request_id }));
      }
    },
  });
}
