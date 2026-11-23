import React, { useMemo, useState } from 'react';
import { Image, Linking, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import { Ionicons } from '@expo/vector-icons';
import type { MessageRecord, ToolRecord } from '../protocol/types';
import { usePalette, type Palette } from '../theme/colors';
import {
  cleanUserDisplayText,
  isRtlText,
  parseInlineMarkdown,
  parseMarkdownBlocks,
  type MarkdownBlock,
} from '../utils/markdown';
import { HtmlArtifactCard } from './HtmlArtifactCard';

/**
 * Pui Desktop-style grouped activity disclosure row:
 * Collapses consecutive tool calls & reasoning into ONE quiet line ("ran 4 commands · edited 3 files")
 * that expands smoothly into a clean card with each step, output, and diff.
 */
export function ActivityGroupRow({
  summary,
  tools,
  thinking,
  hasRunning,
  hasError,
  verbose = false,
  onOpenImage,
}: {
  summary: string;
  tools: ToolRecord[];
  thinking: string[];
  hasRunning: boolean;
  hasError: boolean;
  verbose?: boolean;
  onOpenImage: (uri: string, title?: string) => void;
}) {
  const colors = usePalette();
  const [openOverride, setOpenOverride] = useState<boolean | null>(null);
  const open = openOverride ?? verbose;

  return (
    <View style={styles.activityWrap}>
      <Pressable
        onPress={() => setOpenOverride(!open)}
        style={({ pressed }) => [
          styles.activityBar,
          open && {
            backgroundColor: colors.surface,
            borderColor: colors.border,
            borderWidth: StyleSheet.hairlineWidth,
            borderBottomLeftRadius: 0,
            borderBottomRightRadius: 0,
          },
          pressed && { opacity: 0.8 },
        ]}
      >
        <Ionicons
          name={open ? 'chevron-down' : 'chevron-forward'}
          size={13}
          color={colors.dim}
        />
        {hasRunning ? (
          <View style={[styles.actDot, { backgroundColor: colors.accent }]} />
        ) : hasError ? (
          <View style={[styles.actDot, { backgroundColor: colors.red }]} />
        ) : null}
        <Text numberOfLines={1} style={[styles.activitySummary, { color: colors.muted }]}>
          {summary}
        </Text>
      </Pressable>

      {open ? (
        <View
          style={[
            styles.activityBody,
            { backgroundColor: colors.surface, borderColor: colors.border },
          ]}
        >
          {thinking.map((thought, idx) => (
            <Text
              key={`think-${idx}`}
              selectable
              style={[
                styles.thinkingText,
                {
                  color: colors.muted,
                  borderLeftColor: colors.border,
                  textAlign: isRtlText(thought) ? 'right' : 'left',
                },
              ]}
            >
              {thought}
            </Text>
          ))}
          <View style={styles.toolList}>
            {tools.map((tool, idx) => (
              <ToolItem
                key={tool.id || `${tool.name}-${idx}`}
                tool={tool}
                defaultOpen={verbose}
                onOpenImage={onOpenImage}
              />
            ))}
          </View>
        </View>
      ) : null}
    </View>
  );
}

interface Props {
  message: MessageRecord;
  inlineTools?: ToolRecord[];
  onOpenImage: (uri: string, title?: string) => void;
  onOpenFileTarget: (target: string) => void;
}

export function MessageBubble({
  message,
  inlineTools,
  onOpenImage,
  onOpenFileTarget,
}: Props) {
  const colors = usePalette();
  const isUser = message.role === 'user';

  const toolsToShow = inlineTools ?? message.tools;
  const visualTools = toolsToShow.filter((t) => Boolean(t.visual));
  const openTools = toolsToShow.filter((t) => Boolean(t.open_target));

  if (isUser) {
    const displayText = cleanUserDisplayText(message.text);
    const rtl = isRtlText(displayText);
    const singleImage = message.images.length === 1;
    return (
      <View style={styles.userRow}>
        <View
          style={[
            styles.userBubble,
            { backgroundColor: colors.userBubble, borderColor: colors.border },
          ]}
        >
          {displayText ? (
            <Text
              selectable
              style={[
                styles.userText,
                {
                  color: colors.text,
                  textAlign: rtl ? 'right' : 'left',
                  writingDirection: rtl ? 'rtl' : 'ltr',
                },
              ]}
            >
              {displayText}
            </Text>
          ) : null}
          {message.images.length > 0 ? (
            <ScrollView
              horizontal
              showsHorizontalScrollIndicator={false}
              style={[styles.imageStrip, !displayText && { marginTop: 0 }]}
              contentContainerStyle={styles.imageStripContent}
            >
              {message.images.map((img, idx) => {
                const uri = `data:${img.mime || 'image/png'};base64,${img.data}`;
                const label = img.file?.name ?? `Image ${idx + 1}`;
                return (
                  <Pressable
                    key={idx}
                    onPress={() => onOpenImage(uri, label)}
                    style={[
                      singleImage ? styles.singleThumbWrap : styles.multiThumbWrap,
                      { backgroundColor: colors.codeBg, borderColor: colors.border },
                    ]}
                  >
                    <Image
                      source={{ uri }}
                      style={styles.thumbFill}
                      resizeMode="cover"
                    />
                  </Pressable>
                );
              })}
            </ScrollView>
          ) : null}
        </View>
      </View>
    );
  }

  return (
    <View style={styles.assistantRow}>
      {message.text ? <MarkdownView text={message.text} colors={colors} /> : null}

      {message.images.length > 0 ? (
        <View style={styles.imageGrid}>
          {message.images.map((img, idx) => {
            const uri = `data:${img.mime || 'image/png'};base64,${img.data}`;
            const label = img.file?.name ?? `Image ${idx + 1}`;
            return (
              <Pressable key={idx} onPress={() => onOpenImage(uri, label)}>
                <Image
                  source={{ uri }}
                  style={[styles.thumb, { borderColor: colors.border }]}
                  resizeMode="cover"
                />
              </Pressable>
            );
          })}
        </View>
      ) : null}

      {visualTools.map((tool, idx) =>
        tool.visual ? (
          <HtmlArtifactCard
            key={tool.id || `visual-${idx}`}
            title={tool.visual.title}
            html={tool.visual.html}
            initialHeight={tool.visual.height}
          />
        ) : null,
      )}

      {openTools.map((tool, idx) =>
        tool.open_target ? (
          <Pressable
            key={tool.id || `open-${idx}`}
            onPress={() => onOpenFileTarget(tool.open_target!)}
            style={[
              styles.artifactChip,
              { backgroundColor: colors.surface, borderColor: colors.border },
            ]}
          >
            <Ionicons name="document-text-outline" size={15} color={colors.accent} />
            <Text numberOfLines={1} style={[styles.artifactChipText, { color: colors.text }]}>
              {tool.open_target}
            </Text>
            <Ionicons name="chevron-forward" size={14} color={colors.dim} />
          </Pressable>
        ) : null,
      )}

      {message.error ? (
        <View style={[styles.errorBox, { backgroundColor: colors.redBg, borderColor: colors.red }]}>
          <Text style={{ color: colors.red, fontSize: 12.5, textAlign: 'left' }}>
            {message.error}
          </Text>
        </View>
      ) : null}
    </View>
  );
}

function MarkdownView({ text, colors }: { text: string; colors: Palette }) {
  const blocks = useMemo(() => parseMarkdownBlocks(text), [text]);
  return (
    <View style={styles.mdRoot}>
      {blocks.map((block, idx) => (
        <MarkdownBlockItem key={idx} block={block} colors={colors} />
      ))}
    </View>
  );
}

function InlineText({
  text,
  colors,
  baseSize = 14.5,
  bold = false,
}: {
  text: string;
  colors: Palette;
  baseSize?: number;
  bold?: boolean;
}) {
  const segments = useMemo(() => parseInlineMarkdown(text), [text]);
  const rtl = isRtlText(text);
  return (
    <Text
      selectable
      style={{
        color: colors.text,
        fontSize: baseSize,
        lineHeight: Math.round(baseSize * 1.48),
        fontWeight: bold ? '700' : '400',
        textAlign: rtl ? 'right' : 'left',
        writingDirection: rtl ? 'rtl' : 'ltr',
      }}
    >
      {segments.map((seg, i) => {
        if (seg.type === 'bold') {
          return (
            <Text key={i} style={{ fontWeight: '700', color: colors.text }}>
              {seg.text}
            </Text>
          );
        }
        if (seg.type === 'italic') {
          return (
            <Text key={i} style={{ fontStyle: 'italic' }}>
              {seg.text}
            </Text>
          );
        }
        if (seg.type === 'strike') {
          return (
            <Text key={i} style={{ textDecorationLine: 'line-through', color: colors.muted }}>
              {seg.text}
            </Text>
          );
        }
        if (seg.type === 'code' || seg.type === 'bold_code') {
          return (
            <Text
              key={i}
              style={{
                fontFamily: 'monospace',
                fontWeight: seg.type === 'bold_code' ? '700' : '400',
                fontSize: Math.max(12, baseSize - 1.5),
                backgroundColor: colors.inlineCodeBg,
                color: colors.inlineCodeText,
              }}
            >
              {` ${seg.text} `}
            </Text>
          );
        }
        if (seg.type === 'link') {
          return (
            <Text
              key={i}
              onPress={() => {
                if (/^https?:\/\//i.test(seg.url)) {
                  void Linking.openURL(seg.url).catch(() => {});
                }
              }}
              style={{ color: colors.accent, textDecorationLine: 'underline' }}
            >
              {seg.label}
            </Text>
          );
        }
        return <Text key={i}>{seg.text}</Text>;
      })}
    </Text>
  );
}

function MarkdownBlockItem({ block, colors }: { block: MarkdownBlock; colors: Palette }) {
  if (block.type === 'hr') {
    return <View style={[styles.hr, { backgroundColor: colors.hairline }]} />;
  }
  if (block.type === 'heading') {
    const size = block.level === 1 ? 19 : block.level === 2 ? 17 : 15.5;
    return (
      <View style={styles.headingWrap}>
        <InlineText text={block.text} colors={colors} baseSize={size} bold />
      </View>
    );
  }
  if (block.type === 'code') {
    return (
      <View
        style={[
          styles.codeBlock,
          { backgroundColor: colors.codeBg, borderColor: colors.border },
        ]}
      >
        {block.lang ? (
          <Text style={[styles.codeLang, { color: colors.dim }]}>{block.lang}</Text>
        ) : null}
        <ScrollView horizontal showsHorizontalScrollIndicator={false}>
          <Text selectable style={[styles.codeText, { color: colors.text }]}>
            {block.code}
          </Text>
        </ScrollView>
      </View>
    );
  }
  if (block.type === 'blockquote') {
    return (
      <View
        style={[
          styles.blockquote,
          { borderLeftColor: colors.accent, backgroundColor: colors.surface },
        ]}
      >
        <InlineText text={block.text} colors={colors} baseSize={13.5} />
      </View>
    );
  }
  if (block.type === 'bullet') {
    return (
      <View style={styles.listWrap}>
        {block.items.map((item, idx) => (
          <View key={idx} style={styles.listRow}>
            <Text style={[styles.bulletGlyph, { color: colors.muted }]}>•</Text>
            <View style={{ flex: 1 }}>
              <InlineText text={item} colors={colors} />
            </View>
          </View>
        ))}
      </View>
    );
  }
  if (block.type === 'ordered') {
    return (
      <View style={styles.listWrap}>
        {block.items.map((item, idx) => (
          <View key={idx} style={styles.listRow}>
            <Text style={[styles.numGlyph, { color: colors.muted }]}>{item.num}.</Text>
            <View style={{ flex: 1 }}>
              <InlineText text={item.text} colors={colors} />
            </View>
          </View>
        ))}
      </View>
    );
  }
  if (block.type === 'table') {
    return (
      <ScrollView
        horizontal
        showsHorizontalScrollIndicator={false}
        style={[styles.tableWrap, { borderColor: colors.border, backgroundColor: colors.surface }]}
      >
        <View>
          <View style={[styles.tableRow, { backgroundColor: colors.surfaceElevated }]}>
            {block.headers.map((h, i) => (
              <View key={i} style={[styles.tableCell, { borderColor: colors.hairline }]}>
                <InlineText text={h} colors={colors} baseSize={12.5} bold />
              </View>
            ))}
          </View>
          {block.rows.map((row, rIdx) => (
            <View
              key={rIdx}
              style={[
                styles.tableRow,
                { borderTopWidth: StyleSheet.hairlineWidth, borderTopColor: colors.hairline },
              ]}
            >
              {row.map((cell, cIdx) => (
                <View key={cIdx} style={[styles.tableCell, { borderColor: colors.hairline }]}>
                  <InlineText text={cell} colors={colors} baseSize={12.5} />
                </View>
              ))}
            </View>
          ))}
        </View>
      </ScrollView>
    );
  }
  return <InlineText text={block.text} colors={colors} />;
}

function ToolItem({
  tool,
  defaultOpen = false,
  onOpenImage,
}: {
  tool: ToolRecord;
  defaultOpen?: boolean;
  onOpenImage: (uri: string, title?: string) => void;
}) {
  const colors = usePalette();
  const [openOverride, setOpenOverride] = useState<boolean | null>(null);
  const open = openOverride ?? defaultOpen;
  const subtitle = tool.path || tool.command || tool.args;
  return (
    <View style={[styles.toolCard, { backgroundColor: colors.codeBg, borderColor: colors.border }]}>
      <Pressable onPress={() => setOpenOverride(!open)} style={styles.toolHeader}>
        <Text
          style={[
            styles.toolName,
            { color: tool.failed ? colors.red : tool.finished ? colors.green : colors.amber },
          ]}
        >
          {tool.name}
        </Text>
        <Text numberOfLines={1} style={[styles.toolSub, { color: colors.muted }]}>
          {subtitle}
        </Text>
        {tool.added > 0 || tool.removed > 0 ? (
          <Text style={[styles.diffCounts, { color: colors.dim }]}>
            +{tool.added}/-{tool.removed}
          </Text>
        ) : null}
      </Pressable>
      {open && (tool.diff || tool.output) ? (
        <Text selectable style={[styles.toolBody, { color: colors.text }]}>
          {(tool.diff || tool.output).slice(0, 4000)}
        </Text>
      ) : null}
      {tool.images.length > 0 ? (
        <View style={styles.imageGrid}>
          {tool.images.map((img, idx) => {
            const uri = `data:${img.mime || 'image/png'};base64,${img.data}`;
            return (
              <Pressable key={idx} onPress={() => onOpenImage(uri, `${tool.name} image`)}>
                <Image
                  source={{ uri }}
                  style={[styles.thumb, { borderColor: colors.border }]}
                  resizeMode="cover"
                />
              </Pressable>
            );
          })}
        </View>
      ) : null}
    </View>
  );
}

function summarizeTools(tools: ToolRecord[]): string {
  const counts = new Map<string, number>();
  for (const t of tools) {
    counts.set(t.name, (counts.get(t.name) ?? 0) + 1);
  }
  return Array.from(counts.entries())
    .map(([name, n]) => (n > 1 ? `${name} ×${n}` : name))
    .join(' · ');
}

const styles = StyleSheet.create({
  userRow: {
    paddingHorizontal: 14,
    marginVertical: 6,
    alignItems: 'flex-end',
    direction: 'ltr',
  },
  userBubble: {
    borderWidth: StyleSheet.hairlineWidth,
    borderRadius: 18,
    paddingHorizontal: 14,
    paddingVertical: 10,
    maxWidth: '88%',
    overflow: 'hidden',
  },
  userText: {
    fontSize: 14.5,
    lineHeight: 21,
  },
  assistantRow: {
    paddingHorizontal: 16,
    marginVertical: 7,
    direction: 'ltr',
  },
  activityWrap: {
    paddingHorizontal: 14,
    marginVertical: 4,
    direction: 'ltr',
  },
  activityBar: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
    paddingHorizontal: 10,
    paddingVertical: 7,
    borderRadius: 10,
  },
  actDot: {
    width: 6,
    height: 6,
    borderRadius: 3,
    flexShrink: 0,
  },
  activitySummary: {
    fontSize: 12.5,
    fontWeight: '500',
    flex: 1,
    textAlign: 'left',
  },
  activityBody: {
    borderWidth: StyleSheet.hairlineWidth,
    borderTopWidth: 0,
    borderBottomLeftRadius: 10,
    borderBottomRightRadius: 10,
    padding: 10,
    gap: 6,
  },
  metaRow: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    marginBottom: 6,
  },
  roleLabel: {
    fontSize: 11,
    fontWeight: '600',
  },
  metaRight: {
    fontSize: 10.5,
  },
  mdRoot: {
    gap: 8,
  },
  headingWrap: {
    marginTop: 6,
    marginBottom: 2,
  },
  hr: {
    height: 1,
    marginVertical: 6,
  },
  blockquote: {
    borderLeftWidth: 3,
    paddingHorizontal: 10,
    paddingVertical: 7,
    borderRadius: 6,
  },
  listWrap: {
    gap: 5,
  },
  listRow: {
    flexDirection: 'row',
    alignItems: 'flex-start',
    gap: 8,
  },
  bulletGlyph: {
    fontSize: 15,
    lineHeight: 21,
    width: 12,
    textAlign: 'center',
  },
  numGlyph: {
    fontSize: 13.5,
    lineHeight: 21,
    minWidth: 20,
    fontWeight: '600',
  },
  tableWrap: {
    borderWidth: 1,
    borderRadius: 10,
  },
  tableRow: {
    flexDirection: 'row',
  },
  tableCell: {
    paddingHorizontal: 10,
    paddingVertical: 7,
    minWidth: 105,
    maxWidth: 220,
    borderRightWidth: StyleSheet.hairlineWidth,
  },
  codeBlock: {
    borderWidth: StyleSheet.hairlineWidth,
    borderRadius: 10,
    padding: 10,
  },
  codeLang: {
    fontSize: 10,
    fontWeight: '700',
    marginBottom: 5,
    textTransform: 'uppercase',
  },
  codeText: {
    fontFamily: 'monospace',
    fontSize: 12,
    lineHeight: 18,
    textAlign: 'left',
  },
  collapsePill: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 5,
    paddingHorizontal: 10,
    paddingVertical: 6,
    borderRadius: 999,
    borderWidth: StyleSheet.hairlineWidth,
    marginBottom: 8,
    alignSelf: 'flex-start',
  },
  collapseText: {
    fontSize: 12,
    fontWeight: '600',
  },
  thinkingText: {
    fontSize: 12.5,
    lineHeight: 18,
    fontStyle: 'italic',
    marginBottom: 10,
    borderLeftWidth: 2,
    paddingLeft: 10,
  },
  toolsWrap: {
    marginBottom: 4,
  },
  toolList: {
    gap: 5,
    marginBottom: 8,
  },
  toolCard: {
    borderWidth: StyleSheet.hairlineWidth,
    borderRadius: 9,
    padding: 8,
  },
  toolHeader: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
  },
  toolName: {
    fontFamily: 'monospace',
    fontSize: 11.5,
    fontWeight: '700',
  },
  toolSub: {
    fontFamily: 'monospace',
    fontSize: 11,
    flex: 1,
    textAlign: 'left',
  },
  diffCounts: {
    fontFamily: 'monospace',
    fontSize: 10.5,
  },
  toolBody: {
    fontFamily: 'monospace',
    fontSize: 11,
    lineHeight: 16,
    marginTop: 6,
    textAlign: 'left',
  },
  imageGrid: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    gap: 8,
    marginTop: 8,
  },
  imageStrip: {
    marginTop: 8,
    flexGrow: 0,
  },
  imageStripContent: {
    flexDirection: 'row',
    gap: 8,
  },
  singleThumbWrap: {
    width: 148,
    height: 176,
    borderRadius: 12,
    borderWidth: StyleSheet.hairlineWidth,
    overflow: 'hidden',
  },
  multiThumbWrap: {
    width: 96,
    height: 128,
    borderRadius: 10,
    borderWidth: StyleSheet.hairlineWidth,
    overflow: 'hidden',
  },
  thumbFill: {
    width: '100%',
    height: '100%',
  },
  thumb: {
    width: 124,
    height: 156,
    borderRadius: 10,
    borderWidth: 1,
  },
  artifactChip: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 7,
    paddingHorizontal: 11,
    paddingVertical: 9,
    borderRadius: 10,
    borderWidth: 1,
    marginTop: 8,
  },
  artifactChipText: {
    fontSize: 12.5,
    flex: 1,
    textAlign: 'left',
  },
  errorBox: {
    marginTop: 8,
    padding: 10,
    borderRadius: 10,
    borderWidth: StyleSheet.hairlineWidth,
  },
});
