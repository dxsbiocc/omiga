export const EDITABLE_EMPTY_SENTINEL = "\u200B";

export function normalizeEditableText(text: string): string {
  return text.split(EDITABLE_EMPTY_SENTINEL).join("");
}

export interface EditableInputUpdate {
  nextValue: string;
  shouldCommit: boolean;
  shouldNormalizeDom: boolean;
}

export function getEditableInputUpdate(
  rawValue: string,
  isComposing: boolean,
): EditableInputUpdate {
  const nextValue = normalizeEditableText(rawValue);
  if (isComposing) {
    return {
      nextValue,
      shouldCommit: false,
      shouldNormalizeDom: false,
    };
  }
  return {
    nextValue,
    shouldCommit: true,
    shouldNormalizeDom: nextValue !== rawValue || nextValue === "",
  };
}
