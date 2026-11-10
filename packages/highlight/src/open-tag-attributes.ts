import type { Input, SyntaxNode } from "@lezer/common";

export function getOpenTagAttributes(node: SyntaxNode, input: Input): Record<string, string> {
  const attributes: Record<string, string> = Object.create(null);
  const openTag = node.getChild("OpenTag");
  if (!openTag) return attributes;

  for (const attribute of openTag.getChildren("Attribute")) {
    const name = attribute.getChild("AttributeName");
    if (!name) continue;
    const value =
      attribute.getChild("AttributeValue") || attribute.getChild("UnquotedAttributeValue");
    const key = input.read(name.from, name.to).toLowerCase();
    attributes[key] = value ? input.read(value.from, value.to).replace(/^["']|["']$/g, "") : "";
  }
  return attributes;
}
