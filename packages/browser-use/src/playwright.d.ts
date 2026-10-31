/**
 * Optional peer: Playwright is never a hard dependency (CI must stay lean).
 * The live adapter loads it dynamically and degrades gracefully when absent.
 */
declare module "playwright";
