import { describe, expect, it } from "vitest";
import { resolveMarkdownImageReference } from "./markdownImage";

describe("resolveMarkdownImageReference", () => {
  it("blocks inline base64 images so large payloads are not rendered in chat", () => {
    expect(
      resolveMarkdownImageReference("data:image/png;base64,AAAA", "/work"),
    ).toEqual({
      kind: "blocked-inline",
      rawSrc: "data:image/png;base64,AAAA",
    });
  });

  it("keeps remote image URLs unchanged", () => {
    expect(
      resolveMarkdownImageReference("https://example.test/figure.png", "/work"),
    ).toEqual({
      kind: "remote",
      rawSrc: "https://example.test/figure.png",
      src: "https://example.test/figure.png",
    });
  });

  it("resolves file URLs to local paths and preserves cache suffixes", () => {
    expect(
      resolveMarkdownImageReference(
        "file:///Users/me/project/operator-results/run/figure.png?v=1#plot",
        "/work",
      ),
    ).toEqual({
      kind: "local",
      rawSrc:
        "file:///Users/me/project/operator-results/run/figure.png?v=1#plot",
      localPath: "/Users/me/project/operator-results/run/figure.png",
      suffix: "?v=1#plot",
    });
  });

  it("resolves workspace-relative output images under the current workspace", () => {
    expect(
      resolveMarkdownImageReference(
        "operator-results/viz_heatmap/figure.png",
        "/Users/me/project/",
      ),
    ).toEqual({
      kind: "local",
      rawSrc: "operator-results/viz_heatmap/figure.png",
      localPath: "/Users/me/project/operator-results/viz_heatmap/figure.png",
      suffix: "",
    });
  });

  it("resolves bare image names but leaves the final existence check to the loader", () => {
    expect(resolveMarkdownImageReference("figure.png", "/Users/me/project")).toEqual({
      kind: "local",
      rawSrc: "figure.png",
      localPath: "/Users/me/project/figure.png",
      suffix: "",
    });
  });
});
