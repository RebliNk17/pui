import React from 'react';
import { Image, Modal, Pressable, StyleSheet, Text, View } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { Ionicons } from '@expo/vector-icons';

interface Props {
  uri: string | null;
  title?: string;
  onClose: () => void;
}

export function ImageModal({ uri, title, onClose }: Props) {
  return (
    <Modal visible={Boolean(uri)} transparent animationType="fade" onRequestClose={onClose}>
      <SafeAreaView style={styles.backdrop}>
        <View style={styles.topBar}>
          <Text numberOfLines={1} style={styles.title}>
            {title ?? 'Image preview'}
          </Text>
          <Pressable onPress={onClose} style={styles.closeBtn} hitSlop={10}>
            <Ionicons name="close" size={24} color="#fff" />
          </Pressable>
        </View>
        <Pressable style={styles.stage} onPress={onClose}>
          {uri ? (
            <Image source={{ uri }} style={styles.image} resizeMode="contain" />
          ) : null}
        </Pressable>
      </SafeAreaView>
    </Modal>
  );
}

const styles = StyleSheet.create({
  backdrop: {
    flex: 1,
    backgroundColor: 'rgba(0,0,0,0.96)',
    direction: 'ltr',
  },
  topBar: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: 16,
    paddingVertical: 10,
  },
  title: {
    color: '#fff',
    fontSize: 14,
    fontWeight: '600',
    flex: 1,
    marginRight: 12,
  },
  closeBtn: {
    width: 36,
    height: 36,
    borderRadius: 18,
    backgroundColor: 'rgba(255,255,255,0.14)',
    alignItems: 'center',
    justifyContent: 'center',
  },
  stage: {
    flex: 1,
    paddingHorizontal: 8,
    paddingBottom: 16,
  },
  image: {
    flex: 1,
    width: '100%',
  },
});
