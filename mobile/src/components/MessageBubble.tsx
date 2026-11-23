import React, { useState } from 'react';
import { Image, Pressable, StyleSheet, Text, View } from 'react-native';
import { Ionicons } from '@expo/vector-icons';
import type { MessageRecord, ToolRecord } from '../protocol/types';
import { usePalette } from '../theme/colors';
import { HtmlArtifactCard } from './HtmlArtifactCard';

interface Props {
  message: MessageRecord;
  onOpenImage: (uri: string, title?: string) => void;
  onOpenFileTarget: (target: string) => void;
}

export function MessageBubble({ message, onOpenImage, onOpenFileTarget }: Props) {
  const colors = usePalette();
  const isUser = message.role === 'user';
  const [showThinking, setShowThinking] = useState(false);
  const [showTools, setShowTools] = useState(false);

  const visualTools = message.tools.filter((t) => Boolean(t.visual));
  const openTools = message.tools.filter((t) => Boolean(t.open_target));
  const activityTools = message.tools.filter((t) => !t.visual);

  return (
    <View style={[styles.row, isUser ? styles.rowUser : styles.rowAssistant]}>
      <View
        style={[
          styles.bubble,
          isUser
            ? { backgroundColor: colors.accentBg, borderColor: colors.accent }
            : { backgroundColor: colors.surface, borderColor: colors.border },
        ]}
      >
        <View style={styles.metaRow}>
          <Text style={[styles.roleLabel, { color: isUser ? colors.accent : colors.muted }]}>
            {isUser ? 'You' : message.model || 'Pi'}
          </Text>
          {message.cost > 0 ? (
            <Text style={[styles.metaRight, { color: colors.dim }]}>
              ${message.cost.toFixed(4)}
            </Text>
          ) : null}
        </View>

        {message.thinking ? (
          <Pressable
            onPress={() => setShowThinking((v) => !v)}
            style={[styles.collapsePill, { backgroundColor: colors.surfaceElevated }]}
          >
            <Ionicons
              name={showThinking ? 'chevron-down' : 'chevron-forward'}
              size={12}
              color={colors.muted}
            />
            <Text style={[styles.collapseText, { color: colors.muted }]}>
              Reasoning ({message.thinking.length} chars)
            </Text>
          </Pressable>
        ) : null}
        {showThinking && message.thinking ? (
          <Text selectable style={[styles.thinkingText, { color: colors.muted }]}>
            {message.thinking}
          </Text>
        ) : null}

        {activityTools.length > 0 ? (
          <View style={styles.toolsWrap}>
            <Pressable
              onPress={() => setShowTools((v) => !v)}
              style={[styles.collapsePill, { backgroundColor: colors.surfaceElevated }]}
            >
              <Ionicons
                name={showTools ? 'chevron-down' : 'chevron-forward'}
                size={12}
                color={colors.muted}
              />
              <Text style={[styles.collapseText, { color: colors.muted }]}>
                {summarizeTools(activityTools)}
              </Text>
            </Pressable>
            {showTools ? (
              <View style={styles.toolList}>
                {activityTools.map((tool) => (
                  <ToolItem
                    key={tool.id || `${tool.name}-${tool.path}-${tool.command}`}
                    tool={tool}
                    onOpenImage={onOpenImage}
                  />
                ))}
              </View>
            ) : null}
          </View>
        ) : null}

        {message.text ? <FormattedText text={message.text} /> : null}

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
                { backgroundColor: colors.surfaceElevated, borderColor: colors.border },
              ]}
            >
              <Ionicons name="document-text-outline" size={14} color={colors.accent} />
              <Text numberOfLines={1} style={[styles.artifactChipText, { color: colors.text }]}>
                Open artifact: {tool.open_target}
              </Text>
              <Ionicons name="chevron-forward" size={14} color={colors.dim} />
            </Pressable>
          ) : null,
        )}

        {message.error ? (
          <View style={[styles.errorBox, { backgroundColor: colors.redBg }]}>
            <Text style={{ color: colors.red, fontSize: 12 }}>{message.error}</Text>
          </View>
        ) : null}
      </View>
    </View>
  );
}

function ToolItem({
  tool,
  onOpenImage,
}: {
  tool: ToolRecord;
  onOpenImage: (uri: string, title?: string) => void;
}) {
  const colors = usePalette();
  const [open, setOpen] = useState(false);
  const subtitle = tool.path || tool.command || tool.args;
  return (
    <View style={[styles.toolCard, { backgroundColor: colors.codeBg, borderColor: colors.border }]}>
      <Pressable onPress={() => setOpen((v) => !v)} style={styles.toolHeader}>
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

/**
 * Lightweight Markdown-aware renderer separating fenced code blocks from prose.
 */
function FormattedText({ text }: { text: string }) {
  const colors = usePalette();
  const parts = text.split(/(```[\s\S]*?```)/g);
  return (
    <View style={{ gap: 6 }}>
      {parts.map((part, idx) => {
        if (!part) return null;
        if (part.startsWith('```') && part.endsWith('```')) {
          const inner = part.slice(3, -3);
          const firstNl = inner.indexOf('\n');
          const lang = firstNl > 0 ? inner.slice(0, firstNl).trim() : '';
          const code = firstNl > 0 ? inner.slice(firstNl + 1) : inner;
          return (
            <View
              key={idx}
              style={[styles.codeBlock, { backgroundColor: colors.codeBg, borderColor: colors.border }]}
            >
              {lang ? (
                <Text style={[styles.codeLang, { color: colors.dim }]}>{lang}</Text>
              ) : null}
              <Text selectable style={[styles.codeText, { color: colors.text }]}>
                {code.trimEnd()}
              </Text>
            </View>
          );
        }
        return (
          <Text key={idx} selectable style={[styles.prose, { color: colors.text }]}>
            {part.trim()}
          </Text>
        );
      })}
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
  row: {
    paddingHorizontal: 12,
    marginVertical: 5,
  },
  rowUser: {
    alignItems: 'flex-end',
  },
  rowAssistant: {
    alignItems: 'stretch',
  },
  bubble: {
    borderWidth: 1,
    borderRadius: 12,
    padding: 11,
    maxWidth: '100%',
  },
  metaRow: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    marginBottom: 4,
  },
  roleLabel: {
    fontSize: 11,
    fontWeight: '700',
  },
  metaRight: {
    fontSize: 10.5,
  },
  prose: {
    fontSize: 14,
    lineHeight: 20,
  },
  codeBlock: {
    borderWidth: StyleSheet.hairlineWidth,
    borderRadius: 8,
    padding: 8,
  },
  codeLang: {
    fontSize: 10,
    fontWeight: '600',
    marginBottom: 4,
    textTransform: 'uppercase',
  },
  codeText: {
    fontFamily: 'monospace',
    fontSize: 12,
    lineHeight: 17,
  },
  collapsePill: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 5,
    paddingHorizontal: 8,
    paddingVertical: 5,
    borderRadius: 7,
    marginBottom: 6,
    alignSelf: 'flex-start',
  },
  collapseText: {
    fontSize: 11.5,
    fontWeight: '600',
  },
  thinkingText: {
    fontSize: 12,
    lineHeight: 17,
    fontStyle: 'italic',
    marginBottom: 8,
  },
  toolsWrap: {
    marginBottom: 4,
  },
  toolList: {
    gap: 5,
    marginBottom: 6,
  },
  toolCard: {
    borderWidth: StyleSheet.hairlineWidth,
    borderRadius: 8,
    padding: 7,
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
  },
  diffCounts: {
    fontFamily: 'monospace',
    fontSize: 10.5,
  },
  toolBody: {
    fontFamily: 'monospace',
    fontSize: 11,
    lineHeight: 15,
    marginTop: 6,
  },
  imageGrid: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    gap: 6,
    marginTop: 8,
  },
  thumb: {
    width: 112,
    height: 112,
    borderRadius: 8,
    borderWidth: 1,
  },
  artifactChip: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
    paddingHorizontal: 10,
    paddingVertical: 8,
    borderRadius: 8,
    borderWidth: 1,
    marginTop: 8,
  },
  artifactChipText: {
    fontSize: 12.5,
    flex: 1,
  },
  errorBox: {
    marginTop: 8,
    padding: 8,
    borderRadius: 8,
  },
});
