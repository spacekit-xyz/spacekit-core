import { LLM_STATUS, type LlmAdapter, type LlmStatus, type CapturedLlmRequest } from "../host.js";

export type LlmChatMessage = {
  role: "user" | "assistant" | "system";
  content: string;
};

/** Default repetition penalty to reduce repetitive loops in small models. */
const DEFAULT_REPETITION_PENALTY = 1.15;

export interface LlmChatEngine {
  chat: {
    completions: {
      create: (params: {
        messages: LlmChatMessage[];
        stream?: boolean;
        temperature?: number;
        max_tokens?: number;
        repetition_penalty?: number;
      }) => Promise<{
        choices: Array<{ message: { content: string } }>;
      }>;
    };
  };
  unload?: () => Promise<void>;
}

/**
 * TRM (Tiny Recursive Model) configuration for recursive reasoning.
 * Enables small models to achieve better results through multi-step refinement.
 */
export interface TRMConfig {
  /** Maximum reasoning steps (default: 3) */
  maxSteps: number;
  /** Enable chain-of-thought prompting (default: true) */
  chainOfThought: boolean;
  /** Enable self-verification step (default: true) */
  selfVerify: boolean;
  /** Memory buffer size for context accumulation (default: 2048 chars) */
  memoryBufferSize: number;
  /** Temperature decay per step (multiplier, default: 0.9) */
  temperatureDecay: number;
  /** Confidence threshold to stop early (0-1, default: 0.8) */
  confidenceThreshold: number;
}

const DEFAULT_TRM_CONFIG: TRMConfig = {
  maxSteps: 3,
  chainOfThought: true,
  selfVerify: true,
  memoryBufferSize: 2048,
  temperatureDecay: 0.9,
  confidenceThreshold: 0.8,
};

/**
 * Cached LLM adapter with TRM (Tiny Recursive Model) support.
 * Applies recursive reasoning to improve small model outputs.
 * Precompute must be called before contract execution.
 */
export class WebLlmAdapter implements LlmAdapter {
  private engine: LlmChatEngine | null = null;
  private status: LlmStatus = LLM_STATUS.NOT_LOADED;
  private cachedResponse = "";
  private trmConfig: TRMConfig = { ...DEFAULT_TRM_CONFIG };
  private reasoningMemory: string[] = [];
  
  // Two-phase execution support
  private captureMode = false;
  private capturedRequest: CapturedLlmRequest | null = null;

  setEngine(engine: LlmChatEngine | null) {
    this.engine = engine;
    this.status = engine ? LLM_STATUS.READY : LLM_STATUS.NOT_LOADED;
  }

  getEngine(): LlmChatEngine | null {
    return this.engine;
  }

  setStatus(status: LlmStatus) {
    this.status = status;
  }

  getStatus(): LlmStatus {
    return this.status;
  }

  /**
   * Enable/disable capture mode for two-phase contract execution.
   * In capture mode, infer() records the contract's prompt/params but returns empty.
   */
  setCaptureMode(enabled: boolean): void {
    this.captureMode = enabled;
    if (enabled) {
      this.capturedRequest = null;
    }
  }

  /**
   * Get the captured LLM request from the last capture-mode execution.
   */
  getCapturedRequest(): CapturedLlmRequest | null {
    return this.capturedRequest;
  }

  /**
   * Clear any captured request.
   */
  clearCapturedRequest(): void {
    this.capturedRequest = null;
  }

  /**
   * Configure TRM recursive reasoning parameters.
   */
  setTRMConfig(config: Partial<TRMConfig>) {
    this.trmConfig = { ...this.trmConfig, ...config };
  }

  getTRMConfig(): TRMConfig {
    return { ...this.trmConfig };
  }

  /**
   * Clear reasoning memory between tasks.
   */
  clearMemory() {
    this.reasoningMemory = [];
  }

  /**
   * Standard single-pass inference (for simple queries).
   * Splits the contract prompt into system/user roles so small models
   * don't echo system instructions back as content.
   */
  async precompute(prompt: string, maxTokens: number, temperature: number): Promise<string> {
    if (!this.engine) {
      throw new Error("LLM not loaded");
    }
    const userQuestion = this.extractUserQuestion(prompt);
    const systemInstructions = this.extractSystemInstructions(prompt);
    const messages: LlmChatMessage[] = [];
    if (systemInstructions) {
      messages.push({ role: "system", content: systemInstructions });
    }
    messages.push({ role: "user", content: userQuestion });
    const result = await this.engine.chat.completions.create({
      messages,
      temperature: temperature / 100,
      max_tokens: maxTokens,
      repetition_penalty: DEFAULT_REPETITION_PENALTY,
    });
    this.cachedResponse = result.choices[0]?.message?.content || "";
    return this.cachedResponse;
  }

  /**
   * TRM-enhanced recursive reasoning for complex tasks.
   * Runs multiple refinement passes to improve output quality.
   */
  async precomputeTRM(
    prompt: string,
    maxTokens: number,
    temperature: number,
    config?: Partial<TRMConfig>
  ): Promise<{ response: string; steps: number; confidence: number }> {
    if (!this.engine) {
      throw new Error("LLM not loaded");
    }

    const cfg = { ...this.trmConfig, ...config };
    let currentResponse = "";
    let currentTemp = temperature / 100;
    let step = 0;
    let confidence = 0;

    // Build context from memory buffer
    const memoryContext = this.reasoningMemory.length > 0
      ? `Previous context:\n${this.reasoningMemory.slice(-3).join("\n---\n")}\n\n`
      : "";

    // Extract the user's actual question from the full contract prompt.
    // Contract prompts typically wrap user input inside system instructions,
    // e.g. "You are Kit...\n\nUser: <question>\n\nKit:"
    // We need just the user question for refinement/verification so the model
    // doesn't re-echo system instructions on every TRM step.
    const userQuestion = this.extractUserQuestion(prompt);
    const systemInstructions = this.extractSystemInstructions(prompt);

    // Build proper chat messages with system/user roles so the model
    // doesn't echo system instructions back as content.
    const systemMessage = systemInstructions
      ? `${memoryContext}${systemInstructions}`
      : memoryContext
        ? `${memoryContext}You are a helpful assistant.`
        : "You are a helpful assistant.";

    let userMessage: string;
    if (cfg.chainOfThought && !systemInstructions) {
      userMessage = `Think step by step about this task:\n\n${userQuestion}\n\nFirst, analyze the problem. Then provide your answer.`;
    } else {
      userMessage = userQuestion;
    }

    const initialResult = await this.engine.chat.completions.create({
      messages: [
        { role: "system", content: systemMessage },
        { role: "user", content: userMessage },
      ],
      temperature: currentTemp,
      max_tokens: maxTokens,
      repetition_penalty: DEFAULT_REPETITION_PENALTY,
    });
    currentResponse = initialResult.choices[0]?.message?.content || "";
    step = 1;

    // Steps 2-N: Recursive refinement
    // Use the extracted user question (not the full system+user prompt) so the
    // model focuses on improving the answer content rather than re-echoing
    // system instructions.
    while (step < cfg.maxSteps) {
      currentTemp *= cfg.temperatureDecay;
      
      const refinePrompt = `The user asked: "${userQuestion}"\n\nYour previous answer was:\n"${currentResponse}"\n\nImprove this answer. Be more accurate, complete, and concise. Output only the improved answer.`;
      
      const refineResult = await this.engine.chat.completions.create({
        messages: [{ role: "user", content: refinePrompt }],
        temperature: currentTemp,
        max_tokens: maxTokens,
        repetition_penalty: DEFAULT_REPETITION_PENALTY,
      });
      
      const refinedResponse = refineResult.choices[0]?.message?.content || "";
      
      // Check if refinement is substantially different (indicates improvement)
      const similarity = this.computeSimilarity(currentResponse, refinedResponse);
      confidence = similarity > 0.9 ? Math.min(confidence + 0.3, 1.0) : confidence + 0.1;
      
      // Early stopping if response stabilized (high confidence)
      if (similarity > 0.95 && confidence >= cfg.confidenceThreshold) {
        break;
      }
      
      currentResponse = refinedResponse;
      step++;
    }

    // Optional: Self-verification step
    // Ask the model to check for factual errors and output the final answer
    // directly, rather than asking a yes/no question that small models answer
    // literally ("Yes, this answer is correct.").
    if (cfg.selfVerify && step < cfg.maxSteps + 1) {
      const verifyPrompt = `The user asked: "${userQuestion}"\n\nProposed answer:\n"${currentResponse}"\n\nCheck this answer for factual errors or omissions. Output the corrected and final answer. Do not say whether it is correct — just output the best possible answer.`;
      
      const verifyResult = await this.engine.chat.completions.create({
        messages: [{ role: "user", content: verifyPrompt }],
        temperature: 0.1, // Low temperature for verification
        max_tokens: maxTokens,
        repetition_penalty: DEFAULT_REPETITION_PENALTY,
      });
      
      const verification = verifyResult.choices[0]?.message?.content || "";
      const similarity = this.computeSimilarity(currentResponse, verification);
      if (similarity > 0.85) {
        // Answer is stable after verification — high confidence
        confidence = Math.min(confidence + 0.2, 1.0);
      }
      if (verification.length > 10) {
        currentResponse = verification;
      }
      step++;
    }

    // Store in memory buffer for future context
    const memoryEntry = `Task: ${prompt.slice(0, 100)}...\nResult: ${currentResponse.slice(0, 200)}...`;
    this.reasoningMemory.push(memoryEntry);
    if (this.reasoningMemory.join("\n").length > cfg.memoryBufferSize) {
      this.reasoningMemory.shift();
    }

    this.cachedResponse = currentResponse;
    return { response: currentResponse, steps: step, confidence };
  }

  /**
   * Extract the user's actual question from a contract prompt.
   * Contract prompts wrap user input in system instructions, e.g.:
   *   "You are Kit, a helpful AI assistant...\n\nUser: What are derivatives?\n\nKit:"
   * This extracts "What are derivatives?" so TRM refinement steps don't
   * re-inject the system instructions.
   */
  private extractUserQuestion(prompt: string): string {
    // Try common patterns: "User: <question>\n" or "USER: <question>\n"
    const userMatch = prompt.match(/(?:User|USER):\s*([\s\S]*?)(?:\n\s*(?:Kit|KIT|Assistant|ASSISTANT):|\s*$)/);
    if (userMatch && userMatch[1].trim().length > 0) {
      return userMatch[1].trim();
    }
    // Try "Content: <text>" pattern (analyze/classify)
    const contentMatch = prompt.match(/Content:\s*([\s\S]*?)$/);
    if (contentMatch && contentMatch[1].trim().length > 0) {
      return contentMatch[1].trim();
    }
    // Fallback: return last meaningful line or the whole prompt truncated
    const lines = prompt.trim().split("\n").filter(l => l.trim().length > 0);
    return lines[lines.length - 1] || prompt.slice(0, 500);
  }

  /**
   * Extract system instructions from a contract prompt.
   * Returns the text before "User:" / "USER:" / "Content:", or null if
   * no structured prompt is detected.
   */
  private extractSystemInstructions(prompt: string): string | null {
    // Match everything before "User:" or "USER:"
    const userSplit = prompt.match(/^([\s\S]*?)(?:User|USER):\s/);
    if (userSplit && userSplit[1].trim().length > 10) {
      return userSplit[1].trim();
    }
    // Match everything before "Content:"
    const contentSplit = prompt.match(/^([\s\S]*?)Content:\s/);
    if (contentSplit && contentSplit[1].trim().length > 10) {
      return contentSplit[1].trim();
    }
    return null;
  }

  /**
   * Compute rough similarity between two strings (0-1).
   */
  private computeSimilarity(a: string, b: string): number {
    if (a === b) return 1.0;
    if (!a || !b) return 0.0;
    
    const wordsA = new Set(a.toLowerCase().split(/\s+/));
    const wordsB = new Set(b.toLowerCase().split(/\s+/));
    const intersection = [...wordsA].filter(w => wordsB.has(w)).length;
    const union = new Set([...wordsA, ...wordsB]).size;
    
    return union > 0 ? intersection / union : 0;
  }

  infer(prompt: string, maxTokens: number, temperature: number): string {
    // Capture mode: record the contract's request but return empty
    if (this.captureMode) {
      this.capturedRequest = { prompt, maxTokens, temperature };
      return ""; // Return empty to signal "pending"
    }
    
    // Normal mode: return cached response
    if (this.cachedResponse) {
      const response = this.cachedResponse;
      this.cachedResponse = "";
      return response;
    }
    return "";
  }
}
