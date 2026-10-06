// Answers to meeting invitations given from the island, by invite id. Memory only. The toast,
// the Calendar tab and the Notifications tab all show the same answer state from here.

import { useSyncExternalStore } from "react";
import { ipc, type InviteResponse } from "../ipc";

export type InviteAnswer = { state: "sending"; response: InviteResponse } | { state: "done"; response: InviteResponse } | { state: "failed"; response: InviteResponse };

type Send = (id: string, response: InviteResponse) => Promise<boolean>;

export interface InviteAnswers {
  /** Answer once; a second answer while one is being sent is ignored. Resolves to whether Outlook took it. */
  respond(id: string, response: InviteResponse): Promise<boolean>;
  get(id: string): InviteAnswer | undefined;
  subscribe(listener: () => void): () => void;
  getSnapshot(): ReadonlyMap<string, InviteAnswer>;
}

export function createInviteAnswers(send: Send = ipc.outlookRespondInvite): InviteAnswers {
  let answers: ReadonlyMap<string, InviteAnswer> = new Map();
  const listeners = new Set<() => void>();
  const set = (id: string, answer: InviteAnswer) => {
    answers = new Map(answers).set(id, answer);
    listeners.forEach((listener) => listener());
  };
  return {
    async respond(id, response) {
      const current = answers.get(id);
      if (current?.state === "sending" || current?.state === "done") return current.state === "done";
      set(id, { state: "sending", response });
      let ok = false;
      try {
        ok = await send(id, response);
      } catch {
        ok = false;
      }
      set(id, { state: ok ? "done" : "failed", response });
      return ok;
    },
    get: (id) => answers.get(id),
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    getSnapshot: () => answers,
  };
}

export const inviteAnswers = createInviteAnswers();

export function useInviteAnswer(id: string, store: InviteAnswers = inviteAnswers): InviteAnswer | undefined {
  const all = useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot);
  return all.get(id);
}

/** Invites answered from the island (successfully), so lists can drop them before the next sync. */
export function useAnsweredInviteIds(store: InviteAnswers = inviteAnswers): ReadonlySet<string> {
  const all = useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot);
  const done = new Set<string>();
  all.forEach((answer, id) => {
    if (answer.state === "done") done.add(id);
  });
  return done;
}
