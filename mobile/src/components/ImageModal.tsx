import React from 'react';
import { Image, Modal, Pressable, StyleSheet, Text, View } from 'react-native';
import { Ionicons } from '@expo/vector-icons';

interface Props {
  uri: string | null;
  title?: string;
  onClose: () => void;
}

export function ImageModal({ uri, title, onClose }: Props) {
  return (
    <Modal visible={Boolean(uri)} transparent animationType="fade" onRequestClose={onClose}>
      <View style={styles.backdrop}>
        <View style={styles.topBar}>
          <Text numberOfLines={1} style={styles.title}>
            {title ?? 'Image'}
          </Text>
          <Pressable onPress={onClose} style={styles.closeBtn}>
            <Ionicons name="close" size={22} color="#fff" />
          </Pressable>
        </View>
        {uri ? (
          <Image source={{ uri }} style={styles.image} resizeMode="contain" />
        ) : null}
      </View>
    </Modal>
  );
}

const styles = StyleSheet.create({
  backdrop: {
    flex: 1,
    backgroundColor: 'rgba(0,0,0,0.94)',
    paddingTop: 48,
    paddingBottom: 24,
  },
  topBar: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: 16,
    paddingBottom: 12,
  },
  title: {
    color: '#fff',
    fontSize: 14,
    fontWeight: '600',
    flex: 1,
    marginRight: 12,
  },
  closeBtn: {
    padding: 6,
  },
  image: {
    flex: 1,
    width: '100%',
  },
});
