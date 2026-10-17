import { useMemo, type ReactElement } from "react";
import { View } from "react-native";
import { Gesture, GestureDetector } from "react-native-gesture-handler";
import { runOnJS } from "react-native-reanimated";
import type { DraggableRenderItemInfo } from "./draggable-list.types";

type NativeDragHandler = (itemData: Record<string, unknown>, x: number, y: number) => void;

export function SortableInlineList<T>({
  data,
  keyExtractor,
  renderItem,
  disabled = false,
  onNativeDragStart,
  onNativeDragMove,
  onNativeDragEnd,
  onNativeDragCancel,
  getItemData,
}: {
  data: T[];
  keyExtractor: (item: T, index: number) => string;
  renderItem: (info: DraggableRenderItemInfo<T>) => ReactElement;
  onDragEnd?: (data: T[]) => void;
  useDragHandle?: boolean;
  disabled?: boolean;
  externalDndContext?: boolean;
  activeId?: string | null;
  getItemData?: (item: T, index: number) => Record<string, unknown>;
  onNativeDragStart?: NativeDragHandler;
  onNativeDragMove?: NativeDragHandler;
  onNativeDragEnd?: NativeDragHandler;
  onNativeDragCancel?: (itemData: Record<string, unknown>) => void;
}): ReactElement {
  return (
    <>
      {data.map((item, index) => {
        const id = keyExtractor(item, index);
        return (
          <SortableInlineListItem
            key={id}
            item={item}
            index={index}
            renderItem={renderItem}
            itemData={getItemData?.(item, index) ?? { id }}
            disabled={disabled}
            onNativeDragStart={onNativeDragStart}
            onNativeDragMove={onNativeDragMove}
            onNativeDragEnd={onNativeDragEnd}
            onNativeDragCancel={onNativeDragCancel}
          />
        );
      })}
    </>
  );
}

function SortableInlineListItem<T>({
  item,
  index,
  renderItem,
  itemData,
  disabled,
  onNativeDragStart,
  onNativeDragMove,
  onNativeDragEnd,
  onNativeDragCancel,
}: {
  item: T;
  index: number;
  renderItem: (info: DraggableRenderItemInfo<T>) => ReactElement;
  itemData: Record<string, unknown>;
  disabled: boolean;
  onNativeDragStart?: NativeDragHandler;
  onNativeDragMove?: NativeDragHandler;
  onNativeDragEnd?: NativeDragHandler;
  onNativeDragCancel?: (itemData: Record<string, unknown>) => void;
}): ReactElement {
  const gesture = useMemo(() => {
    if (disabled || !onNativeDragStart || !onNativeDragMove || !onNativeDragEnd) {
      return null;
    }

    return Gesture.Pan()
      .activateAfterLongPress(280)
      .shouldCancelWhenOutside(false)
      .onStart((event) => {
        runOnJS(onNativeDragStart)(itemData, event.absoluteX, event.absoluteY);
      })
      .onUpdate((event) => {
        runOnJS(onNativeDragMove)(itemData, event.absoluteX, event.absoluteY);
      })
      .onEnd((event) => {
        runOnJS(onNativeDragEnd)(itemData, event.absoluteX, event.absoluteY);
      })
      .onFinalize((_event, success) => {
        if (!success && onNativeDragCancel) {
          runOnJS(onNativeDragCancel)(itemData);
        }
      });
  }, [
    disabled,
    itemData,
    onNativeDragCancel,
    onNativeDragEnd,
    onNativeDragMove,
    onNativeDragStart,
  ]);

  const info: DraggableRenderItemInfo<T> = {
    item,
    index,
    drag: () => {},
    isActive: false,
  };
  const content = renderItem(info);
  if (!gesture) {
    return content;
  }
  return (
    <GestureDetector gesture={gesture}>
      <View collapsable={false}>{content}</View>
    </GestureDetector>
  );
}
