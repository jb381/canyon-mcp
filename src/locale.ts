import { CanyonError } from "./errors.js";

export const DEFAULT_LOCALE = "en-de";

export function normalizeLocale(input = DEFAULT_LOCALE): string {
  const locale = input.trim().replace("_", "-").toLowerCase();
  if (!/^[a-z]{2}-[a-z]{2}$/.test(locale)) {
    throw new CanyonError(
      `Invalid locale '${input}'. Use Canyon's language-country form, for example en-de or de-de.`,
      "INVALID_LOCALE",
    );
  }
  return locale;
}

export function localeForSitemap(locale: string): string {
  const [language, country] = normalizeLocale(locale).split("-");
  return `${language}_${country!.toUpperCase()}`;
}

export function localePath(locale: string): string {
  return `/${normalizeLocale(locale)}/`;
}
