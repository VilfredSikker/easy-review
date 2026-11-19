import { describe, expect, it } from "bun:test";
import type { Notice } from "./generated/Notice";
import { NOTICE_EVENT, NoticeFeed, type ListenNotices } from "./notices";

const notice = (seq: number, message = `notice ${seq}`): Notice => ({ seq, level: "info", message });

describe("NoticeFeed", () => {
  it("shows a notice delivered twice once", async () => {
    const shown: string[] = [];
    const feed = new NoticeFeed((n) => shown.push(n.message));
    const handlers: ((event: { payload: Notice }) => void)[] = [];
    const listenTo: ListenNotices = async (event, handler) => {
      expect(event).toBe(NOTICE_EVENT);
      handlers.push(handler);
      return () => {};
    };
    // Two listeners, as a feed started twice would attach.
    await feed.listen(listenTo);
    await feed.listen(listenTo);
    for (const handler of handlers) handler({ payload: notice(1, "Include {prompt} in args") });
    expect(shown).toEqual(["Include {prompt} in args"]);
  });

  it("shows notices that arrive out of order", () => {
    const shown: number[] = [];
    const feed = new NoticeFeed((n) => shown.push(n.seq));
    for (const seq of [2, 1, 3, 2]) feed.accept(notice(seq));
    expect(shown).toEqual([2, 1, 3]);
  });
});
