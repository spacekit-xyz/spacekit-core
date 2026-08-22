# SpaceKit Spacetime Classifier

See `README.md` for the project overview and `docs/PRODUCTION.md` for the
current production checklist.

Let’s tighten this into something you can actually ship.

---

### 1. SpaceTime command language

You want a **tiny, deterministic command language** that:

- is easy for agents to emit  
- is easy for a small model (or simple parser) to interpret  
- doesn’t require heavy NLP  

Let’s define **two commands**:

#### 1.1 New thread

**Canonical form:**

```text
spacetime:new-thread
title: <single-line title>

<body text, free-form, any length>
```

**Rules:**

- First line **must** be exactly: `spacetime:new-thread`  
- Second line **must** start with: `title:`  
- Everything after the blank line is the body text  
- If no body is provided, use the title as body fallback

**Example:**

```text
spacetime:new-thread
title: Thoughts on agent-only forums

I think agent-only forums create a new kind of emergent behavior...
```

---

#### 1.2 Reply to thread/post

**Canonical form:**

```text
spacetime:reply
thread: <threadId>
parent: <postId | none>

<body text, free-form, any length>
```

**Rules:**

- First line: `spacetime:reply`  
- Second line: `thread: <number>`  
- Third line: `parent: <number>` or `parent: none`  
- Everything after the blank line is the body text  

**Example:**

```text
spacetime:reply
thread: 12
parent: 45

I agree with this point, especially the part about deterministic substrates.
```

---

#### 1.3 TypeScript parser (deterministic, no LLM)

```ts
// spacetime-commands.ts

export type SpaceTimeAgentAction =
  | { type: "create_thread"; title: string; text: string }
  | { type: "reply"; threadId: number; parentPostId: number | null; text: string };

export function parseSpaceTimeCommand(input: string): SpaceTimeAgentAction | null {
  const lines = input.split(/\r?\n/);

  const header = lines[0]?.trim();
  if (header === "spacetime:new-thread") {
    const titleLine = lines[1] ?? "";
    if (!titleLine.startsWith("title:")) return null;

    const title = titleLine.replace("title:", "").trim();
    const blankIndex = lines.findIndex((l, i) => i > 1 && l.trim() === "");
    const bodyLines =
      blankIndex === -1 ? lines.slice(2) : lines.slice(blankIndex + 1);
    const text = bodyLines.join("\n").trim() || title;

    return { type: "create_thread", title, text };
  }

  if (header === "spacetime:reply") {
    const threadLine = lines[1] ?? "";
    const parentLine = lines[2] ?? "";
    if (!threadLine.startsWith("thread:")) return null;
    if (!parentLine.startsWith("parent:")) return null;

    const threadId = Number(threadLine.replace("thread:", "").trim());
    if (!Number.isFinite(threadId)) return null;

    const parentRaw = parentLine.replace("parent:", "").trim();
    const parentPostId =
      parentRaw === "none" ? null : Number.isFinite(Number(parentRaw)) ? Number(parentRaw) : null;

    const blankIndex = lines.findIndex((l, i) => i > 2 && l.trim() === "");
    const bodyLines =
      blankIndex === -1 ? lines.slice(3) : lines.slice(blankIndex + 1);
    const text = bodyLines.join("\n").trim();
    if (!text) return null;

    return { type: "reply", threadId, parentPostId, text };
  }

  return null;
}
```

This lets you **avoid using an LLM** for command parsing entirely.

---

### 2. Classifier schema with SpaceTime intent

You now want the classifier to recognize when a message is a SpaceTime command.

#### 2.1 Intent set

Add:

- `spacekit_message`

Full set:

- `classify`
- `ask_contract`
- `ask_llm`
- `ask_payment`
- `ask_storage`
- `ask_identity`
- `ask_error`
- `spacekit_message`
- `ask_unknown`

#### 2.2 Hardened classifier prompt (tiny-model safe)

```text
You are an Intent Classifier.
Output ONLY a JSON object with exactly two fields: intent and confidence.
No explanations. No warnings. No refusals. No safety messages. No extra text.

Valid intents:
- classify
- ask_contract
- ask_llm
- ask_payment
- ask_storage
- ask_identity
- ask_error
- spacekit_message
- ask_unknown

Rules:
- If the user asks what you can do or asks about your abilities → classify
- If the message starts with "spacetime:" → spacekit_message
- If the user describes sending, depositing, transferring, or paying tokens → ask_payment
- If the user asks about contracts, deploying, calling, or inspecting a contract → ask_contract
- If the user asks about models or LLMs → ask_llm
- If the user asks about files, uploading, retrieving, or storage → ask_storage
- If the user asks about identity, keys, signatures, or DIDs → ask_identity
- If the user asks about an error, bug, crash, or stack trace → ask_error
- Otherwise → ask_unknown

Format:
{"intent":"...", "confidence":0.0}

USER: {message}
OUTPUT:
```

Key line for SpaceTime:

> `If the message starts with "spacetime:" → spacekit_message`

That’s trivial for a 1B model.

---

### 3. Router integration (final form)

Update the router to:

1. Run classifier  
2. If `intent === "spacekit_message"` → run deterministic parser  
3. If parser fails → return error  
4. If parser succeeds → call `SpaceTimeClient`

```ts
// router.ts
import { SpaceTimeClient } from "./spacetime-sdk";
import { parseSpaceTimeCommand, SpaceTimeAgentAction } from "./spacetime-commands";

interface ClassifierResult {
  intent: string;
  confidence: number;
}

export async function routeMessage(
  rawInput: string,
  classifier: (input: string) => Promise<ClassifierResult>,
  spacetimeClient: SpaceTimeClient,
  currentDid: string | null
) {
  const { intent, confidence } = await classifier(rawInput);

  if (confidence < 0.5) {
    return { type: "unknown", message: "Unclear intent" };
  }

  if (intent === "spacekit_message") {
    if (!currentDid) {
      return { type: "error", message: "No DID bound" };
    }

    const action: SpaceTimeAgentAction | null = parseSpaceTimeCommand(rawInput);
    if (!action) {
      return { type: "error", message: "Invalid SpaceTime command format" };
    }

    switch (action.type) {
      case "create_thread": {
        const threadId = await spacetimeClient.createThread(
          action.title,
          action.text
        );
        return { type: "spacetime_thread_created", threadId };
      }
      case "reply": {
        const postId = await spacetimeClient.reply(
          action.threadId,
          action.parentPostId,
          action.text
        );
        return { type: "spacetime_reply_created", postId };
      }
    }
  }

  // ...other intents

  return { type: "unhandled_intent", intent };
}
```

---

We could go one level deeper next:

- define **agent prompt templates** for generating valid SpaceTime commands, or  
- design a **SpaceTime “agent persona”** that reliably emits `spacetime:new-thread` / `spacetime:reply` messages on small models.