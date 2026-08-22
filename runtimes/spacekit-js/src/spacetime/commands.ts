export type SpaceTimeAgentAction =
  | { type: "create_thread"; title: string; text: string }
  | {
      type: "reply";
      threadId: number;
      parentPostId: number | null;
      text: string;
    };

export function parseSpaceTimeCommand(
  input: string
): SpaceTimeAgentAction | null {
  const lines = input.split(/\r?\n/);
  const header = lines[0]?.trim();

  if (header === "spacetime:new-thread") {
    const titleLine = lines[1] ?? "";
    if (!titleLine.startsWith("title:")) {
      return null;
    }

    const title = titleLine.replace("title:", "").trim();
    if (!title) {
      return null;
    }

    const blankIndex = lines.findIndex(
      (line, idx) => idx > 1 && line.trim() === ""
    );
    const bodyLines =
      blankIndex === -1 ? lines.slice(2) : lines.slice(blankIndex + 1);
    const text = bodyLines.join("\n").trim() || title;

    return { type: "create_thread", title, text };
  }

  if (header === "spacetime:reply") {
    const threadLine = lines[1] ?? "";
    const parentLine = lines[2] ?? "";
    if (!threadLine.startsWith("thread:")) {
      return null;
    }
    if (!parentLine.startsWith("parent:")) {
      return null;
    }

    const threadId = Number(threadLine.replace("thread:", "").trim());
    if (!Number.isFinite(threadId)) {
      return null;
    }

    const parentRaw = parentLine.replace("parent:", "").trim();
    const parentPostId =
      parentRaw === "none"
        ? null
        : Number.isFinite(Number(parentRaw))
        ? Number(parentRaw)
        : null;

    const blankIndex = lines.findIndex(
      (line, idx) => idx > 2 && line.trim() === ""
    );
    const bodyLines =
      blankIndex === -1 ? lines.slice(3) : lines.slice(blankIndex + 1);
    const text = bodyLines.join("\n").trim();
    if (!text) {
      return null;
    }

    return { type: "reply", threadId, parentPostId, text };
  }

  return null;
}
