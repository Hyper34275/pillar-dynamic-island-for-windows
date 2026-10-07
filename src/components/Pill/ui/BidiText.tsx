import { splitBidi } from "../../../design/direction";

/**
 * A text with its technical tokens (URLs, e-mail, Windows paths, file names, IPv4, times, dates)
 * wrapped in <bdi dir="ltr">: real Unicode isolation, so "C:\Users\Daniel\Report.pdf" or
 * "11:00–12:00" keep their own order inside a Hebrew sentence, and the sentence's own punctuation
 * and numbers keep theirs. It renders a fragment of inline children: put it inside the element
 * that carries the paragraph `dir` (textDirection) and the clamp / truncate classes; ellipsis and
 * line-clamp work on inline children as on plain text. See design/direction.ts for the model.
 */
export function BidiText({ text }: { text: string | null | undefined }) {
  const segments = splitBidi(text);
  if (!segments.some((segment) => segment.ltr)) return <>{text ?? ""}</>;
  return (
    <>
      {segments.map((segment, i) =>
        segment.ltr ? (
          <bdi key={i} dir="ltr">
            {segment.text}
          </bdi>
        ) : (
          segment.text
        )
      )}
    </>
  );
}
