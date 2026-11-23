import React, { useMemo, useState } from 'react';
import { Pressable, ScrollView, StyleSheet, Text, TextInput, View } from 'react-native';
import { Ionicons } from '@expo/vector-icons';
import { usePalette } from '../theme/colors';
import {
  buildApprovalResponse,
  parseApprovalDialog,
  parseQuestionsDialog,
  type ParsedQuestionsRequest,
} from '../utils/dialogs';

interface Props {
  dialog: Record<string, unknown>;
  onRespond: (dialogId: string, response: Record<string, unknown>) => Promise<void>;
}

export function DialogCard({ dialog, onRespond }: Props) {
  const colors = usePalette();
  const [busy, setBusy] = useState(false);
  const [textInput, setTextInput] = useState(
    typeof dialog.prefill === 'string' ? dialog.prefill : '',
  );

  const approval = useMemo(() => parseApprovalDialog(dialog), [dialog]);
  const questions = useMemo(() => parseQuestionsDialog(dialog), [dialog]);

  if (questions) {
    return (
      <QuestionsFlow
        request={questions}
        busy={busy}
        onSubmit={async (payload) => {
          setBusy(true);
          try {
            await onRespond(questions.dialogId, { value: JSON.stringify(payload) });
          } finally {
            setBusy(false);
          }
        }}
      />
    );
  }

  if (approval) {
    const submit = async (decision: 'allow_once' | 'allow_always' | 'deny') => {
      setBusy(true);
      try {
        await onRespond(approval.id, buildApprovalResponse(approval, decision));
      } finally {
        setBusy(false);
      }
    };

    const prioritizedKeys = [
      ...['command', 'path', 'code'].filter((k) => k in approval.args),
      ...Object.keys(approval.args).filter((k) => !['command', 'path', 'code'].includes(k)),
    ];

    return (
      <View
        style={[
          styles.card,
          { backgroundColor: colors.surface, borderColor: colors.amber },
        ]}
      >
        <View style={styles.kickerRow}>
          <Ionicons name="shield-checkmark-outline" size={14} color={colors.amber} />
          <Text style={[styles.kicker, { color: colors.amber }]}>
            Pui needs your permission
          </Text>
        </View>
        <Text style={[styles.heading, { color: colors.text }]}>{approval.title}</Text>
        {approval.explanation ? (
          <Text style={[styles.body, { color: colors.muted }]}>{approval.explanation}</Text>
        ) : null}
        <ScrollView style={[styles.argsBox, { backgroundColor: colors.codeBg }]}>
          {prioritizedKeys.map((key) => {
            const val = approval.args[key];
            const rendered = typeof val === 'string' ? val : JSON.stringify(val, null, 2);
            return (
              <View key={key} style={styles.argItem}>
                <Text style={[styles.argKey, { color: colors.dim }]}>{key}</Text>
                <Text selectable style={[styles.argVal, { color: colors.text }]}>
                  {rendered}
                </Text>
              </View>
            );
          })}
        </ScrollView>
        <View style={styles.actionRow}>
          <Pressable
            disabled={busy}
            onPress={() => submit('deny')}
            style={[styles.btn, { backgroundColor: colors.redBg, borderColor: colors.red }]}
          >
            <Text style={[styles.btnText, { color: colors.red }]}>Deny</Text>
          </Pressable>
          <Pressable
            disabled={busy}
            onPress={() => submit('allow_once')}
            style={[styles.btn, { backgroundColor: colors.accent }]}
          >
            <Text style={[styles.btnText, { color: '#fff' }]}>Allow once</Text>
          </Pressable>
          {approval.supportsAlways ? (
            <Pressable
              disabled={busy}
              onPress={() => submit('allow_always')}
              style={[
                styles.btn,
                { backgroundColor: colors.surfaceElevated, borderColor: colors.border },
              ]}
            >
              <Text style={[styles.btnText, { color: colors.text }]}>Always allow</Text>
            </Pressable>
          ) : null}
        </View>
      </View>
    );
  }

  // Generic confirm / select / input fallback.
  const id = typeof dialog.id === 'string' ? dialog.id : '';
  const method = typeof dialog.method === 'string' ? dialog.method : '';
  const title = typeof dialog.title === 'string' ? dialog.title : 'Agent request';
  const message = typeof dialog.message === 'string' ? dialog.message : '';
  const options = Array.isArray(dialog.options)
    ? dialog.options.filter((o): o is string => typeof o === 'string')
    : [];

  const send = async (response: Record<string, unknown>) => {
    setBusy(true);
    try {
      await onRespond(id, response);
    } finally {
      setBusy(false);
    }
  };

  return (
    <View style={[styles.card, { backgroundColor: colors.surface, borderColor: colors.accent }]}>
      <Text style={[styles.heading, { color: colors.text }]}>{title}</Text>
      {message ? <Text style={[styles.body, { color: colors.muted }]}>{message}</Text> : null}
      {method === 'confirm' ? (
        <View style={styles.actionRow}>
          <Pressable
            disabled={busy}
            onPress={() => send({ confirmed: false })}
            style={[styles.btn, { backgroundColor: colors.surfaceElevated, borderColor: colors.border }]}
          >
            <Text style={[styles.btnText, { color: colors.text }]}>Cancel</Text>
          </Pressable>
          <Pressable
            disabled={busy}
            onPress={() => send({ confirmed: true })}
            style={[styles.btn, { backgroundColor: colors.accent }]}
          >
            <Text style={[styles.btnText, { color: '#fff' }]}>Confirm</Text>
          </Pressable>
        </View>
      ) : method === 'select' ? (
        <View style={styles.optionList}>
          {options.map((opt) => (
            <Pressable
              key={opt}
              disabled={busy}
              onPress={() => send({ value: opt })}
              style={[
                styles.optionBtn,
                { backgroundColor: colors.surfaceElevated, borderColor: colors.border },
              ]}
            >
              <Text style={{ color: colors.text, fontSize: 13 }}>{opt}</Text>
            </Pressable>
          ))}
        </View>
      ) : (
        <View>
          <TextInput
            value={textInput}
            onChangeText={setTextInput}
            placeholder="Enter response..."
            placeholderTextColor={colors.dim}
            style={[
              styles.input,
              { backgroundColor: colors.codeBg, color: colors.text, borderColor: colors.border },
            ]}
          />
          <View style={styles.actionRow}>
            <Pressable
              disabled={busy}
              onPress={() => send({ cancelled: true })}
              style={[styles.btn, { backgroundColor: colors.surfaceElevated, borderColor: colors.border }]}
            >
              <Text style={[styles.btnText, { color: colors.text }]}>Cancel</Text>
            </Pressable>
            <Pressable
              disabled={busy}
              onPress={() => send({ value: textInput })}
              style={[styles.btn, { backgroundColor: colors.accent }]}
            >
              <Text style={[styles.btnText, { color: '#fff' }]}>Submit</Text>
            </Pressable>
          </View>
        </View>
      )}
    </View>
  );
}

function QuestionsFlow({
  request,
  busy,
  onSubmit,
}: {
  request: ParsedQuestionsRequest;
  busy: boolean;
  onSubmit: (payload: Record<string, unknown>) => Promise<void>;
}) {
  const colors = usePalette();
  const [step, setStep] = useState(0);
  const [answers, setAnswers] = useState<
    Record<string, { selections: string[]; customText: string; skipped: boolean }>
  >({});

  const q = request.questions[step];
  const current = answers[q.id] ?? { selections: [], customText: '', skipped: false };

  const setCurrent = (next: { selections: string[]; customText: string; skipped: boolean }) => {
    setAnswers((prev) => ({ ...prev, [q.id]: next }));
  };

  const toggleOption = (label: string) => {
    if (q.multiSelect) {
      const exists = current.selections.includes(label);
      const nextSel = exists
        ? current.selections.filter((s) => s !== label)
        : [...current.selections, label];
      setCurrent({ ...current, selections: nextSel, skipped: false });
    } else {
      const nextAns = { ...answers, [q.id]: { selections: [label], customText: '', skipped: false } };
      setAnswers(nextAns);
      if (step < request.questions.length - 1) {
        setStep(step + 1);
      } else {
        finish(nextAns);
      }
    }
  };

  const finish = (
    finalMap: Record<string, { selections: string[]; customText: string; skipped: boolean }>,
  ) => {
    const formatted = request.questions.map((item) => {
      const a = finalMap[item.id] ?? { selections: [], customText: '', skipped: true };
      const custom = a.customText.trim();
      const selections = custom ? [...a.selections, custom] : a.selections;
      return {
        id: item.id,
        answer: selections.join(', '),
        selections,
        custom: Boolean(custom),
        skipped: a.skipped || selections.length === 0,
      };
    });
    void onSubmit({
      version: 1,
      request_id: request.request_id,
      cancelled: false,
      answers: formatted,
    });
  };

  return (
    <View style={[styles.card, { backgroundColor: colors.surface, borderColor: colors.accent }]}>
      <View style={styles.kickerRow}>
        <Ionicons name="help-circle-outline" size={14} color={colors.accent} />
        <Text style={[styles.kicker, { color: colors.accent }]}>
          Question {step + 1} of {request.questions.length}
        </Text>
      </View>
      <Text style={[styles.heading, { color: colors.text }]}>{q.question}</Text>
      <View style={styles.optionList}>
        {q.options.map((opt) => {
          const selected = current.selections.includes(opt.label);
          return (
            <Pressable
              key={opt.label}
              disabled={busy}
              onPress={() => toggleOption(opt.label)}
              style={[
                styles.optionBtn,
                {
                  backgroundColor: selected ? colors.accentBg : colors.surfaceElevated,
                  borderColor: selected ? colors.accent : colors.border,
                },
              ]}
            >
              <Text style={{ color: colors.text, fontSize: 13, fontWeight: '600' }}>
                {opt.label}
              </Text>
              {opt.description ? (
                <Text style={{ color: colors.muted, fontSize: 11.5, marginTop: 2 }}>
                  {opt.description}
                </Text>
              ) : null}
            </Pressable>
          );
        })}
      </View>
      <TextInput
        value={current.customText}
        onChangeText={(text) => setCurrent({ ...current, customText: text, skipped: false })}
        placeholder="Or type something else..."
        placeholderTextColor={colors.dim}
        style={[
          styles.input,
          { backgroundColor: colors.codeBg, color: colors.text, borderColor: colors.border },
        ]}
      />
      <View style={styles.actionRow}>
        {step > 0 ? (
          <Pressable
            disabled={busy}
            onPress={() => setStep(step - 1)}
            style={[styles.btn, { backgroundColor: colors.surfaceElevated, borderColor: colors.border }]}
          >
            <Text style={[styles.btnText, { color: colors.text }]}>Back</Text>
          </Pressable>
        ) : null}
        <Pressable
          disabled={busy}
          onPress={() => {
            const next = { ...answers, [q.id]: { selections: [], customText: '', skipped: true } };
            setAnswers(next);
            if (step < request.questions.length - 1) setStep(step + 1);
            else finish(next);
          }}
          style={[styles.btn, { backgroundColor: colors.surfaceElevated, borderColor: colors.border }]}
        >
          <Text style={[styles.btnText, { color: colors.muted }]}>Skip</Text>
        </Pressable>
        <Pressable
          disabled={busy}
          onPress={() => {
            if (step < request.questions.length - 1) setStep(step + 1);
            else finish(answers);
          }}
          style={[styles.btn, { backgroundColor: colors.accent }]}
        >
          <Text style={[styles.btnText, { color: '#fff' }]}>
            {step < request.questions.length - 1 ? 'Next' : 'Submit'}
          </Text>
        </Pressable>
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  card: {
    borderWidth: 1,
    borderRadius: 12,
    padding: 12,
    marginHorizontal: 12,
    marginVertical: 6,
  },
  kickerRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 5,
    marginBottom: 4,
  },
  kicker: {
    fontSize: 11,
    fontWeight: '700',
    textTransform: 'uppercase',
    letterSpacing: 0.4,
  },
  heading: {
    fontSize: 14,
    fontWeight: '600',
    marginBottom: 6,
  },
  body: {
    fontSize: 12.5,
    lineHeight: 18,
    marginBottom: 8,
  },
  argsBox: {
    maxHeight: 170,
    borderRadius: 8,
    padding: 8,
    marginBottom: 10,
  },
  argItem: {
    marginBottom: 6,
  },
  argKey: {
    fontSize: 10.5,
    fontWeight: '600',
    marginBottom: 2,
  },
  argVal: {
    fontFamily: 'monospace',
    fontSize: 11.5,
    lineHeight: 16,
  },
  actionRow: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    gap: 8,
    marginTop: 8,
  },
  btn: {
    paddingHorizontal: 12,
    paddingVertical: 8,
    borderRadius: 8,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: 'transparent',
  },
  btnText: {
    fontSize: 12.5,
    fontWeight: '600',
  },
  optionList: {
    gap: 6,
    marginBottom: 8,
  },
  optionBtn: {
    borderWidth: 1,
    borderRadius: 8,
    paddingHorizontal: 10,
    paddingVertical: 8,
  },
  input: {
    borderWidth: 1,
    borderRadius: 8,
    paddingHorizontal: 10,
    paddingVertical: 8,
    fontSize: 13,
  },
});
