import type { Messages } from "../messages";
import { account } from "./account";
import { auth } from "./auth";
import { bots } from "./bots";
import { common } from "./common";
import { configs } from "./configs";
import { returns } from "./returns";

export const zh: Messages = { common, auth, bots, configs, account, returns };
