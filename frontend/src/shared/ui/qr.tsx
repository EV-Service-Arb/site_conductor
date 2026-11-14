import { cn } from "@evinvest/uikit";
import { renderSVG } from "uqr";

/** `text` as a QR code, black on white whatever the theme: a camera reads dark modules on a light ground. */
export function Qr({ text, className }: { text: string; className?: string }) {
  // uqr's SVG holds only module rects: nothing of `text` reaches the markup
  return <div role="img" aria-label={text} className={cn("bg-white [&>svg]:block [&>svg]:size-full", className)} dangerouslySetInnerHTML={{ __html: renderSVG(text, { border: 2 }) }} />;
}
