import { Text } from "react-native";
import {
  forwardRef,
  useCallback,
  useMemo,
  useState,
  type ComponentProps,
  type ReactElement,
  type ReactNode,
} from "react";
import type {
  CodeBlockRendererProps,
  LinkRendererProps,
  MarkdownRenderers,
} from "react-native-enriched-markdown";
import { StyleSheet, withUnistyles } from "react-native-unistyles";
import { useAssistantFileLinkEnvironment, useFileLink } from "@/assistant-file-links";
import { HoverCard, HoverCardContent, useHoverCardTriggerProps } from "@/components/ui/hover-card";
import { Tooltip, TooltipTrigger, TooltipContent } from "@/components/ui/tooltip";
import {
  ForgeLinkCard,
  isForgeReferenceLink,
  useResolveForgeLinkTarget,
  type ForgeLinkTarget,
} from "@/git/forge-link-card";
import { HighlightedCodeBlock } from "@/components/highlighted-code-block";
import type { Theme } from "@/styles/theme";
import { MermaidFence } from "./fence/mermaid";
import { markdownLinkSource } from "./source";

const ThemedCodeBlock = withUnistyles(HighlightedCodeBlock);
const codeStyle = (theme: Theme) => ({
  inheritedStyles: {
    color: theme.colors.foreground,
    fontFamily: theme.fontFamily.mono,
    fontSize: theme.fontSize.code,
  },
  textStyle: { backgroundColor: theme.colors.surface2, padding: 12, borderRadius: 8 },
});
function WebCodeBlock({ code, language }: CodeBlockRendererProps) {
  if (language?.toLowerCase() === "mermaid") return <MermaidFence code={code} />;
  return <ThemedCodeBlock code={code} language={language} uniProps={codeStyle} />;
}

// The library styles links inline, so a stylesheet :hover rule cannot underline them.
const LINK_HOVER_STYLE = { textDecoration: "underline" } as const;

function WebLink({ link, anchorProps, children }: LinkRendererProps) {
  const { tooltipPath, onHoverIn } = useFileLink(markdownLinkSource(link.url, link.sourceType));
  const [hovered, setHovered] = useState(false);
  const handleMouseEnter = useCallback(() => setHovered(true), []);
  const handleMouseLeave = useCallback(() => setHovered(false), []);
  const anchorStyle = useMemo(
    () => ({
      ...anchorProps.style,
      overflowWrap: "anywhere" as const,
      ...(hovered ? LINK_HOVER_STYLE : null),
    }),
    [anchorProps.style, hovered],
  );
  const anchor = (
    <a
      {...anchorProps}
      style={anchorStyle}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
    >
      {children}
    </a>
  );
  const tooltip = (
    <Tooltip delayDuration={400}>
      <TooltipTrigger asChild onHoverIn={onHoverIn}>
        <InlineLinkTrigger>{anchor}</InlineLinkTrigger>
      </TooltipTrigger>
      {tooltipPath ? (
        <TooltipContent side="top" align="start" maxWidth={520}>
          <Text selectable={false} style={styles.tooltipPath}>
            {tooltipPath}
          </Text>
        </TooltipContent>
      ) : null}
    </Tooltip>
  );
  // Only links shaped like an issue or change request carry a card: each card listens for
  // focus and scroll while mounted, which is not worth paying for on every link in a message.
  return isForgeReferenceLink(link.url) ? (
    <ForgeLinkHoverCard href={link.url}>{tooltip}</ForgeLinkHoverCard>
  ) : (
    tooltip
  );
}

/** Long enough that a pointer crossing the text on its way elsewhere opens nothing. */
const FORGE_LINK_CARD_DELAY_MS = 350;

/**
 * Shows what an issue or change-request link points at while the pointer rests on it. The link
 * resolves when the pointer arrives, so one to another repository stays an ordinary link.
 */
function ForgeLinkHoverCard({
  href,
  children,
}: {
  href: string;
  children: ReactNode;
}): ReactElement {
  const environment = useAssistantFileLinkEnvironment();
  const resolveTarget = useResolveForgeLinkTarget();
  const [target, setTarget] = useState<ForgeLinkTarget | null>(null);
  const { serverId, workspaceRoot } = environment;
  const canOpen = useCallback(() => {
    const next = resolveTarget(href, { serverId, cwd: workspaceRoot });
    setTarget(next);
    return next !== null;
  }, [href, resolveTarget, serverId, workspaceRoot]);
  return (
    <HoverCard openDelayMs={FORGE_LINK_CARD_DELAY_MS} canOpen={canOpen}>
      <ForgeLinkHoverTrigger>{children}</ForgeLinkHoverTrigger>
      <HoverCardContent placement="bottom" align="start" offset={6} testID="forge-link-hover-card">
        {target ? <ForgeLinkCard {...target} /> : null}
      </HoverCardContent>
    </HoverCard>
  );
}

// `HoverCardTrigger` renders a View, and a div is invalid inside the library's prose paragraphs.
function ForgeLinkHoverTrigger({ children }: { children: ReactNode }): ReactElement {
  const triggerProps = useHoverCardTriggerProps();
  return <span {...triggerProps}>{children}</span>;
}
// TooltipTrigger also supplies native Pressable handlers; forward only DOM events.
// A View renders a div, which is invalid inside the library's prose paragraphs.
const InlineLinkTrigger = forwardRef<HTMLSpanElement, ComponentProps<"span">>(
  function InlineLinkTrigger({ children, onMouseEnter, onMouseLeave, onFocus, onBlur }, ref) {
    return (
      <span
        ref={ref}
        onMouseEnter={onMouseEnter}
        onMouseLeave={onMouseLeave}
        onFocus={onFocus}
        onBlur={onBlur}
      >
        {children}
      </span>
    );
  },
);
const styles = StyleSheet.create((theme) => ({
  tooltipPath: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.normal,
  },
}));

export const WEB_RENDERERS: MarkdownRenderers = { codeBlock: WebCodeBlock, link: WebLink };
