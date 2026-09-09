import { en } from "./en";
import type { Lang } from "./locale";
import { zh } from "./zh";

// English is the shape of the catalog: a key the zh side misses or invents is a
// type error, so the two languages cannot drift.
export type Messages = typeof en;

export const messages: Record<Lang, Messages> = { en, zh };
