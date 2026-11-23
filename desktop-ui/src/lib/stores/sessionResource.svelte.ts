import { resources } from "./resourceStore";
import { SessionAdapter } from "./sessionAdapter";

/** The app chrome and the AI picker, from the `settings` and `session` resources. */
export const sessionResource = new SessionAdapter({ store: resources });
