import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { SupportQr } from "./SupportQr";

describe("unmodified QR viewport", () => {
  it("clips only the white QR frame from the original image", () => {
    const html = renderToStaticMarkup(<SupportQr />);
    expect(html).toContain('viewBox="154 356 442 442"');
    expect(html).toContain('href="/support-qr.jpg"');
    expect(html).toContain('width="750"');
    expect(html).toContain('height="1148"');
    expect(html).not.toContain("<text");
  });
});
