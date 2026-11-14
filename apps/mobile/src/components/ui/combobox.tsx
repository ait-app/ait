import { nativeComboboxCollisionPadding } from "./combobox-viewport";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";
import type { ReactElement, ReactNode } from "react";
import {
  View,
  Keyboard,
  Text,
  Pressable,
  Modal,
  ScrollView,
  Platform,
  StatusBar,
  useWindowDimensions,
  type LayoutChangeEvent,
  type PressableStateCallbackType,
  type StyleProp,
  type ViewStyle,
} from "react-native";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import type { EditingTextInputHandle } from "@/components/ui/text-input";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { useIsCompactFormFactor } from "@/constants/layout";
import { BottomSheetScrollView } from "@gorhom/bottom-sheet";
import { GestureHandlerRootView } from "react-native-gesture-handler";
import { Check, File, Folder, Search } from "lucide-react-native";
import {
  flip,
  offset as floatingOffset,
  shift,
  size as floatingSize,
  useFloating,
} from "@floating-ui/react-native";
import { getNextActiveIndex } from "./combobox-keyboard";
import { filterAndRankComboboxOptions } from "./combobox-options";
import type { ComboboxOptionModel } from "./combobox-options";
import { isWeb } from "@/constants/platform";
import {
  IsolatedBottomSheetModal,
  SheetVisibleFrame,
  useIsolatedBottomSheetVisibility,
} from "./isolated-bottom-sheet-modal";
import {
  AdaptiveTextInput,
  InlineHeaderView,
  SheetBackground,
  SheetHeaderView,
  type SheetHeader,
} from "@/components/adaptive-modal-sheet";
import { FloatingSurface } from "@/components/ui/floating";
import { useDismissKeyboardOnOpen } from "@/components/ui/keyboard-dismiss";
import {
  getOverlayRoot,
  OverlayLayerProvider,
  useOverlayLayer,
  useWebOverlayRegistration,
} from "@/lib/overlay-root";
import { buildDesktopFrameStyle } from "./combobox-frame-style";

export type ComboboxOption = ComboboxOptionModel;
type ComboboxDesktopPlacement = "top-start" | "bottom-start";

export interface ComboboxProps {
  options: ComboboxOption[];
  value: string;
  onSelect: (id: string) => void;
  renderOption?: (input: {
    option: ComboboxOption;
    selected: boolean;
    active: boolean;
    onPress: () => void;
  }) => ReactElement;
  onSearchQueryChange?: (query: string) => void;
  searchable?: boolean;
  placeholder?: string;
  searchPlaceholder?: string;
  emptyText?: string;
  title?: string;
  /**
   * Structured header. When provided, replaces `title` and
   * is rendered via the shared SheetHeaderView (mobile) / InlineHeaderView
   * (desktop). Built-in search (when `searchable=true` and no `header.search`)
   * is folded into the header so its magnifying glass aligns with the title
   * and any leading icon at the sheet's shared indent.
   */
  header?: SheetHeader;
  mobileChildrenScrollEnabled?: boolean;
  /** Let custom desktop children own scrolling, for example a virtualized list. */
  desktopChildrenScrollEnabled?: boolean;
  /** Overrides the mobile scroll container spacing for custom child content. */
  mobileChildrenContentContainerStyle?: StyleProp<ViewStyle>;
  presentation?: "push" | "replace";
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  desktopPlacement?: ComboboxDesktopPlacement;
  /** Minimum width for the desktop popover (overrides trigger-based width). */
  desktopMinWidth?: number;
  /** Keep the desktop popover at its opening width as its content changes. */
  desktopLockWidth?: boolean;
  /** Fixed height for the desktop popover (overrides default 400px max). */
  desktopFixedHeight?: number;
  /** Content rendered below the scroll area. */
  footer?: ReactNode;
  /** When true, selecting an option does not close the picker (multi-select mode). */
  keepOpenOnSelect?: boolean;
  anchorRef: React.RefObject<View | null>;
  children?: ReactNode;
}

function toNumericStyleValue(value: unknown): number | null {
  if (typeof value === "number" && Number.isFinite(value)) {
    return value;
  }
  if (typeof value === "string") {
    const parsed = Number.parseFloat(value);
    if (Number.isFinite(parsed)) {
      return parsed;
    }
  }
  return null;
}

interface SearchInputProps {
  placeholder: string;
  onChangeText: (text: string) => void;
  autoFocus?: boolean;
  resetKey?: number;
}

function SearchInput({
  placeholder,
  onChangeText,
  autoFocus = false,
  resetKey,
}: SearchInputProps): ReactElement {
  const { theme } = useUnistyles();
  const inputRef = useRef<EditingTextInputHandle>(null);

  useEffect(() => {
    if (autoFocus && isWeb && inputRef.current) {
      const timer = setTimeout(() => {
        inputRef.current?.focus();
      }, 50);
      return () => clearTimeout(timer);
    }
  }, [autoFocus]);

  return (
    <View style={styles.searchInputContainer}>
      <Search size={16} color={theme.colors.foregroundMuted} />
      <AdaptiveTextInput
        ref={inputRef}
        // @ts-expect-error - outlineStyle is web-only
        style={[styles.searchInput, isWeb && { outlineStyle: "none" }]}
        placeholder={placeholder}
        placeholderTextColor={theme.colors.foregroundMuted}
        resetKey={resetKey}
        onChangeText={onChangeText}
        autoCapitalize="none"
        autoCorrect={false}
      />
    </View>
  );
}

interface ComboboxItemProps {
  label: string;
  description?: string;
  kind?: "directory" | "file";
  leadingSlot?: ReactNode;
  trailingSlot?: ReactNode;
  selected?: boolean;
  active?: boolean;
  disabled?: boolean;
  accessibilityLabel?: string;
  onPress: () => void;
  testID?: string;
}

export function ComboboxItem({
  label,
  description,
  kind,
  leadingSlot,
  trailingSlot,
  selected,
  active,
  disabled,
  accessibilityLabel,
  onPress,
  testID,
}: ComboboxItemProps): ReactElement {
  const { theme } = useUnistyles();

  let leadingContent: ReactElement | null = null;
  if (leadingSlot) {
    leadingContent = <View style={styles.comboboxItemLeadingSlot}>{leadingSlot}</View>;
  } else if (kind === "directory") {
    leadingContent = (
      <View style={styles.comboboxItemLeadingSlot}>
        <Folder size={16} color={theme.colors.foregroundMuted} />
      </View>
    );
  } else if (kind === "file") {
    leadingContent = (
      <View style={styles.comboboxItemLeadingSlot}>
        <File size={16} color={theme.colors.foregroundMuted} />
      </View>
    );
  }

  const itemPressableStyle = useCallback(
    ({ pressed, hovered = false }: PressableStateCallbackType & { hovered?: boolean }) => [
      styles.comboboxItem,
      hovered && styles.comboboxItemHovered,
      pressed && styles.comboboxItemPressed,
      active && styles.comboboxItemActive,
      disabled && styles.comboboxItemDisabled,
    ],
    [active, disabled],
  );

  const itemContentStyle = useMemo(
    () => [styles.comboboxItemContent, description && styles.comboboxItemContentInline],
    [description],
  );

  return (
    <Pressable
      testID={testID}
      disabled={disabled}
      onPress={onPress}
      style={itemPressableStyle}
      accessibilityRole="button"
      accessibilityLabel={accessibilityLabel ?? label}
    >
      {leadingContent}
      <View style={itemContentStyle}>
        <Text numberOfLines={1} style={styles.comboboxItemLabel}>
          {label}
        </Text>
        {description ? (
          <Text numberOfLines={1} style={styles.comboboxItemDescription}>
            {description}
          </Text>
        ) : null}
      </View>
      {selected || trailingSlot ? (
        <View style={styles.comboboxItemTrailingContainer}>
          <View style={styles.comboboxItemTrailingSlot}>
            {selected ? <Check size={16} color={theme.colors.foregroundMuted} /> : null}
          </View>
          {trailingSlot}
        </View>
      ) : null}
    </Pressable>
  );
}

function ComboboxEmpty({ children }: { children: ReactNode }): ReactElement {
  return (
    <Text testID="combobox-empty-text" style={styles.emptyText}>
      {children}
    </Text>
  );
}

type RenderOptionFn = NonNullable<ComboboxProps["renderOption"]>;

interface OptionRowProps {
  option: ComboboxOption;
  selected: boolean;
  active: boolean;
  onSelect: (id: string) => void;
  renderOption: RenderOptionFn | undefined;
}

function OptionRow({ option, selected, active, onSelect, renderOption }: OptionRowProps) {
  const handlePress = useCallback(() => onSelect(option.id), [onSelect, option.id]);
  if (renderOption) {
    return <View>{renderOption({ option, selected, active, onPress: handlePress })}</View>;
  }
  return (
    <ComboboxItem
      label={option.label}
      description={option.description}
      kind={option.kind}
      selected={selected}
      active={active}
      onPress={handlePress}
    />
  );
}

interface OptionsListProps {
  options: ComboboxOption[];
  value: string;
  activeIndex: number;
  emptyText: string;
  onSelect: (id: string) => void;
  renderOption: RenderOptionFn | undefined;
}

function OptionsList({
  options,
  value,
  activeIndex,
  emptyText,
  onSelect,
  renderOption,
}: OptionsListProps): ReactElement {
  if (options.length === 0) {
    return <ComboboxEmpty>{emptyText}</ComboboxEmpty>;
  }
  return (
    <>
      {options.map((opt, index) => (
        <OptionRow
          key={opt.id}
          option={opt}
          selected={opt.id === value}
          active={index === activeIndex}
          onSelect={onSelect}
          renderOption={renderOption}
        />
      ))}
    </>
  );
}

interface DesktopPositionInput {
  isMobile: boolean;
  desktopPlacement: ComboboxDesktopPlacement;
  referenceTop: number | null;
  referenceLeft: number | null;
  referenceAtOrigin: boolean;
  desktopContentWidth: number | null;
  windowWidth: number;
  windowHeight: number;
  collisionPadding: number;
  floatingTop: number | null;
  floatingLeft: number | null;
  floatingStyles: ReturnType<typeof useFloating>["floatingStyles"];
  referenceWidth: number | null;
}

interface DesktopPositionResult {
  desktopPositionStyle:
    | { left: number; bottom: number }
    | ReturnType<typeof useFloating>["floatingStyles"];
  hasResolvedDesktopPosition: boolean;
  useMeasuredTopStartPosition: boolean;
}

function computeDesktopPosition(input: DesktopPositionInput): DesktopPositionResult {
  const {
    referenceTop,
    referenceLeft,
    desktopContentWidth,
    windowWidth,
    windowHeight,
    collisionPadding,
    floatingTop,
    floatingLeft,
    floatingStyles,
    referenceAtOrigin,
    referenceWidth,
  } = input;

  if (
    isWeb &&
    !input.isMobile &&
    input.desktopPlacement === "top-start" &&
    referenceTop !== null &&
    referenceLeft !== null &&
    desktopContentWidth !== null
  ) {
    return {
      desktopPositionStyle: {
        left: Math.max(
          collisionPadding,
          Math.min(windowWidth - desktopContentWidth - collisionPadding, referenceLeft),
        ),
        bottom: Math.max(windowHeight - referenceTop + 5, collisionPadding),
      },
      hasResolvedDesktopPosition: referenceWidth !== null && referenceWidth > 0,
      useMeasuredTopStartPosition: true,
    };
  }

  const hasNonZeroFloating = (floatingTop ?? 0) !== 0 || floatingLeft !== 0;
  const floatingReady =
    floatingLeft !== null && floatingTop !== null && (hasNonZeroFloating || referenceAtOrigin);
  return {
    desktopPositionStyle: floatingStyles,
    hasResolvedDesktopPosition: referenceWidth !== null && referenceWidth > 0 && floatingReady,
    useMeasuredTopStartPosition: false,
  };
}

type DesktopKey = "ArrowDown" | "ArrowUp" | "Enter" | "Escape";

interface DesktopKeyHandlerInput {
  orderedVisibleOptions: ComboboxOption[];
  activeIndex: number;
  setActiveIndex: React.Dispatch<React.SetStateAction<number>>;
  handleSelect: (id: string) => void;
  handleClose: () => void;
}

type AvailableSize = { width?: number; height?: number } | null;

function updateReferenceWidth(
  setReferenceWidth: React.Dispatch<React.SetStateAction<number | null>>,
  width: number,
) {
  setReferenceWidth((prev) => {
    if (!(width > 0)) return prev;
    if (prev === width) return prev;
    return width;
  });
}

interface MeasuredAnchorSetters {
  setReferenceLeft: React.Dispatch<React.SetStateAction<number | null>>;
  setReferenceTop: React.Dispatch<React.SetStateAction<number | null>>;
  setReferenceWidth: React.Dispatch<React.SetStateAction<number | null>>;
  setReferenceAtOrigin: React.Dispatch<React.SetStateAction<boolean>>;
}

function useActiveIndexSync(
  isOpen: boolean,
  isMobile: boolean,
  orderedVisibleOptions: ComboboxOption[],
  normalizedSearch: string,
  value: string,
  setActiveIndex: React.Dispatch<React.SetStateAction<number>>,
) {
  useEffect(() => {
    if (!isOpen) return;
    if (!isWeb && isMobile) return;
    setActiveIndex(resolveInitialActiveIndex(orderedVisibleOptions, normalizedSearch, value));
  }, [isMobile, isOpen, normalizedSearch, value, orderedVisibleOptions, setActiveIndex]);
}

function useDesktopFloatingUpdate(
  isOpen: boolean,
  isMobile: boolean,
  orderedVisibleOptionsLength: number,
  searchQuery: string,
  viewportRevision: string,
  update: () => unknown,
) {
  useLayoutEffect(() => {
    if (!isOpen || isMobile) return;
    void update();
    const frame = requestAnimationFrame(() => void update());
    return () => cancelAnimationFrame(frame);
  }, [isOpen, isMobile, orderedVisibleOptionsLength, searchQuery, viewportRevision, update]);
}

function useResetSearchOnOpen(
  isOpen: boolean,
  setSearchQueryWithCallback: (query: string) => void,
  bumpSearchResetKey: () => void,
) {
  useEffect(() => {
    if (isOpen) {
      setSearchQueryWithCallback("");
      bumpSearchResetKey();
    }
  }, [isOpen, setSearchQueryWithCallback, bumpSearchResetKey]);
}

interface DesktopResetSetters {
  setAvailableSize: React.Dispatch<React.SetStateAction<AvailableSize>>;
  setDesktopContentWidth: React.Dispatch<React.SetStateAction<number | null>>;
  setReferenceLeft: React.Dispatch<React.SetStateAction<number | null>>;
  setReferenceWidth: React.Dispatch<React.SetStateAction<number | null>>;
}

function useDesktopPositionReset(
  isOpen: boolean,
  isMobile: boolean,
  desktopPlacement: ComboboxDesktopPlacement,
  update: () => unknown,
  setters: DesktopResetSetters,
) {
  const { setAvailableSize, setDesktopContentWidth, setReferenceLeft, setReferenceWidth } = setters;
  useEffect(() => {
    if (!isOpen || isMobile) {
      setAvailableSize(null);
      setDesktopContentWidth(null);
      setReferenceLeft(null);
      setReferenceWidth(null);
      return;
    }
    const raf = requestAnimationFrame(() => void update());
    return () => cancelAnimationFrame(raf);
  }, [
    desktopPlacement,
    isMobile,
    isOpen,
    update,
    setAvailableSize,
    setDesktopContentWidth,
    setReferenceLeft,
    setReferenceWidth,
  ]);
}

function useAnchorMeasure(
  isOpen: boolean,
  isMobile: boolean,
  anchorRef: React.RefObject<View | null>,
  searchQuery: string,
  windowHeight: number,
  setters: MeasuredAnchorSetters,
) {
  const { setReferenceLeft, setReferenceTop, setReferenceWidth, setReferenceAtOrigin } = setters;
  useEffect(() => {
    if (!isOpen || isMobile) {
      setReferenceLeft(null);
      setReferenceAtOrigin(false);
      setReferenceTop(null);
      return;
    }

    const referenceEl = anchorRef.current;
    if (!referenceEl) {
      setReferenceAtOrigin(false);
      setReferenceTop(null);
      return;
    }

    const measure = () => {
      referenceEl.measureInWindow((x, y, width) => {
        setReferenceLeft((prev) => (prev === x ? prev : x));
        setReferenceAtOrigin(Math.abs(x) <= 1 && Math.abs(y) <= 1);
        setReferenceTop((prev) => (prev === y ? prev : y));
        updateReferenceWidth(setReferenceWidth, width);
      });
    };

    measure();
    const raf = requestAnimationFrame(measure);
    return () => cancelAnimationFrame(raf);
  }, [
    anchorRef,
    isMobile,
    isOpen,
    searchQuery,
    windowHeight,
    setReferenceLeft,
    setReferenceTop,
    setReferenceWidth,
    setReferenceAtOrigin,
  ]);
}

function computeCollisionPadding(): number {
  const basePadding = 16;
  if (Platform.OS !== "android") return basePadding;
  const statusBarHeight = StatusBar.currentHeight ?? 0;
  return Math.max(basePadding, statusBarHeight + basePadding);
}

interface FloatingMiddlewareInput {
  collisionPadding: number | ReturnType<typeof nativeComboboxCollisionPadding>;
  setAvailableSize: React.Dispatch<React.SetStateAction<AvailableSize>>;
  setReferenceWidth: React.Dispatch<React.SetStateAction<number | null>>;
}

function buildFloatingMiddleware(input: FloatingMiddlewareInput) {
  const { collisionPadding, setAvailableSize, setReferenceWidth } = input;
  return [
    floatingOffset(isWeb ? 5 : 4),
    ...(isWeb ? [] : [flip({ padding: collisionPadding })]),
    shift({ padding: collisionPadding }),
    floatingSize({
      padding: collisionPadding,
      apply({ availableWidth, availableHeight, rects }) {
        setAvailableSize((prev) => {
          if (prev && prev.width === availableWidth && prev.height === availableHeight) {
            return prev;
          }
          return { width: availableWidth, height: availableHeight };
        });
        updateReferenceWidth(setReferenceWidth, rects.reference.width);
      },
    }),
  ];
}

function isDesktopKey(key: string): key is DesktopKey {
  return key === "ArrowDown" || key === "ArrowUp" || key === "Enter" || key === "Escape";
}

function dispatchDesktopKey(
  input: DesktopKeyHandlerInput,
  key: DesktopKey,
  event: KeyboardEvent,
): boolean {
  const { orderedVisibleOptions, activeIndex } = input;
  if (key === "ArrowDown" || key === "ArrowUp") {
    event.preventDefault();
    input.setActiveIndex((currentIndex) =>
      getNextActiveIndex({ currentIndex, itemCount: orderedVisibleOptions.length, key }),
    );
    return true;
  }
  if (key === "Enter") {
    if (orderedVisibleOptions.length === 0) return false;
    event.preventDefault();
    const index = activeIndex >= 0 && activeIndex < orderedVisibleOptions.length ? activeIndex : 0;
    input.handleSelect(orderedVisibleOptions[index].id);
    return true;
  }
  event.preventDefault();
  input.handleClose();
  return true;
}

function resolveInitialActiveIndex(
  orderedVisibleOptions: ComboboxOption[],
  normalizedSearch: string,
  value: string,
): number {
  if (orderedVisibleOptions.length === 0) return -1;
  if (normalizedSearch) return 0;
  const selectedIndex = orderedVisibleOptions.findIndex((opt) => opt.id === value);
  return selectedIndex >= 0 ? selectedIndex : 0;
}

type BottomSheetVisibility = ReturnType<typeof useIsolatedBottomSheetVisibility>;

interface MobileBodyProps {
  bottomSheetRef: BottomSheetVisibility["sheetRef"];
  snapPoints: string[];
  handleSheetChange: BottomSheetVisibility["handleSheetChange"];
  handleSheetDismiss: BottomSheetVisibility["handleSheetDismiss"];
  handleIndicatorStyle: { backgroundColor: string };
  titleColor: string;
  title: string;
  header: SheetHeader | undefined;
  onClose: () => void;
  footer: ReactNode;
  searchable: boolean;
  hasChildren: boolean;
  mobileChildrenScrollEnabled: boolean;
  mobileChildrenContentContainerStyle: StyleProp<ViewStyle>;
  presentation?: "push" | "replace";
  searchResetKey: number;
  searchPlaceholder: string;
  setSearchQueryWithCallback: (query: string) => void;
  orderedVisibleOptions: ComboboxOption[];
  value: string;
  activeIndex: number;
  emptyText: string;
  handleSelect: (id: string) => void;
  renderOption: RenderOptionFn | undefined;
  children: ReactNode;
  safeAreaBottom: number;
}

function MobileComboboxBody(props: MobileBodyProps): ReactElement {
  const comboboxTitleStyle = useMemo(
    () => [styles.comboboxTitle, { color: props.titleColor }],
    [props.titleColor],
  );
  const frameStyle = useMemo(
    () => [styles.mobileSheetFrame, { paddingBottom: props.safeAreaBottom }],
    [props.safeAreaBottom],
  );

  const body = props.hasChildren ? (
    props.children
  ) : (
    <OptionsList
      options={props.orderedVisibleOptions}
      value={props.value}
      activeIndex={props.activeIndex}
      emptyText={props.emptyText}
      onSelect={props.handleSelect}
      renderOption={props.renderOption}
    />
  );

  return (
    <IsolatedBottomSheetModal
      ref={props.bottomSheetRef}
      contextBridge={null}
      snapPoints={props.snapPoints}
      index={0}
      enableDynamicSizing={false}
      onChange={props.handleSheetChange}
      onDismiss={props.handleSheetDismiss}
      backdropOpacity={0.45}
      enablePanDownToClose
      backgroundComponent={SheetBackground}
      handleIndicatorStyle={props.handleIndicatorStyle}
      keyboardBehavior="extend"
      keyboardBlurBehavior="none"
      presentation={props.presentation}
    >
      <SheetVisibleFrame>
        <View style={frameStyle}>
          {props.header ? (
            <SheetHeaderView header={props.header} onClose={props.onClose} />
          ) : (
            <>
              <View style={styles.bottomSheetHeader}>
                <Text key={props.titleColor} style={comboboxTitleStyle}>
                  {props.title}
                </Text>
              </View>
              {!props.hasChildren && props.searchable ? (
                <SearchInput
                  placeholder={props.searchPlaceholder}
                  onChangeText={props.setSearchQueryWithCallback}
                  resetKey={props.searchResetKey}
                />
              ) : null}
            </>
          )}
          {props.hasChildren && !props.mobileChildrenScrollEnabled ? (
            body
          ) : (
            <BottomSheetScrollView
              style={styles.mobileSheetBody}
              contentContainerStyle={[
                styles.comboboxScrollContent,
                props.mobileChildrenContentContainerStyle,
              ]}
              keyboardShouldPersistTaps="handled"
              showsVerticalScrollIndicator={false}
            >
              {body}
            </BottomSheetScrollView>
          )}
          {props.footer ? <View style={styles.footer}>{props.footer}</View> : null}
        </View>
      </SheetVisibleFrame>
    </IsolatedBottomSheetModal>
  );
}

interface DesktopBodyProps {
  overlayLayer: number;
  handleClose: () => void;
  handleDesktopKey: (key: DesktopKey, event: KeyboardEvent) => boolean;
  refs: ReturnType<typeof useFloating>["refs"];
  desktopFrameStyle: StyleProp<ViewStyle>;
  handleDesktopContentLayout: (event: LayoutChangeEvent) => void;
  header: SheetHeader | undefined;
  footer: ReactNode;
  searchable: boolean;
  searchPlaceholder: string;
  setSearchQueryWithCallback: (query: string) => void;
  orderedVisibleOptions: ComboboxOption[];
  value: string;
  activeIndex: number;
  emptyText: string;
  handleSelect: (id: string) => void;
  renderOption: RenderOptionFn | undefined;
  hasChildren: boolean;
  childrenScrollEnabled: boolean;
  children: ReactNode;
}

function DesktopComboboxChildrenBody(props: {
  header: SheetHeader | undefined;
  scrollEnabled: boolean;
  children: ReactNode;
}): ReactElement {
  const header = props.header ? <InlineHeaderView header={props.header} /> : null;
  if (!props.scrollEnabled) {
    return (
      <>
        {header}
        <View style={styles.desktopChildrenBody}>{props.children}</View>
      </>
    );
  }
  return (
    <>
      {header}
      <ScrollView
        contentContainerStyle={styles.desktopChildrenScrollContent}
        keyboardShouldPersistTaps="handled"
        showsVerticalScrollIndicator={false}
        style={styles.desktopScroll}
      >
        {props.children}
      </ScrollView>
    </>
  );
}

function DesktopComboboxOptionsBody(props: {
  header: SheetHeader | undefined;
  searchable: boolean;
  searchPlaceholder: string;
  setSearchQueryWithCallback: (query: string) => void;
  orderedVisibleOptions: ComboboxOption[];
  value: string;
  activeIndex: number;
  emptyText: string;
  handleSelect: (id: string) => void;
  renderOption: RenderOptionFn | undefined;
}): ReactElement {
  return (
    <>
      {props.header ? <InlineHeaderView header={props.header} /> : null}
      {props.header || !props.searchable ? null : (
        <SearchInput
          placeholder={props.searchPlaceholder}
          onChangeText={props.setSearchQueryWithCallback}
          autoFocus
        />
      )}
      <ScrollView
        contentContainerStyle={styles.desktopScrollContent}
        keyboardShouldPersistTaps="handled"
        showsVerticalScrollIndicator={false}
        style={styles.desktopScroll}
      >
        <OptionsList
          options={props.orderedVisibleOptions}
          value={props.value}
          activeIndex={props.activeIndex}
          emptyText={props.emptyText}
          onSelect={props.handleSelect}
          renderOption={props.renderOption}
        />
      </ScrollView>
    </>
  );
}

function DesktopComboboxBody(props: DesktopBodyProps): ReactElement {
  const handleDesktopKey = props.handleDesktopKey;
  const handleWebOverlayKeyDown = useCallback(
    (event: KeyboardEvent) => {
      if (!isDesktopKey(event.key)) return false;
      return handleDesktopKey(event.key, event);
    },
    [handleDesktopKey],
  );
  const setWebOverlayScope = useWebOverlayRegistration({
    active: isWeb,
    layer: props.overlayLayer,
    onKeyDown: handleWebOverlayKeyDown,
  });
  const setFloatingRef = useCallback(
    (node: View | null) => {
      props.refs.setFloating(node);
      setWebOverlayScope(node);
    },
    [props.refs, setWebOverlayScope],
  );

  const overlay = (
    <OverlayLayerProvider layer={props.overlayLayer}>
      <View
        ref={props.refs.setOffsetParent}
        collapsable={false}
        style={[
          styles.desktopOverlay,
          isWeb ? styles.desktopOverlayWeb : null,
          isWeb ? { zIndex: props.overlayLayer } : null,
        ]}
      >
        <Pressable style={styles.desktopBackdrop} onPress={props.handleClose} />
        <FloatingSurface
          testID="combobox-desktop-container"
          style={styles.desktopContainer}
          frameStyle={props.desktopFrameStyle}
          ref={setFloatingRef}
          collapsable={false}
          role="dialog"
          aria-modal
          tabIndex={-1}
          onLayout={props.handleDesktopContentLayout}
        >
          {props.hasChildren ? (
            <DesktopComboboxChildrenBody
              header={props.header}
              scrollEnabled={props.childrenScrollEnabled}
            >
              {props.children}
            </DesktopComboboxChildrenBody>
          ) : (
            <DesktopComboboxOptionsBody
              header={props.header}
              searchable={props.searchable}
              searchPlaceholder={props.searchPlaceholder}
              setSearchQueryWithCallback={props.setSearchQueryWithCallback}
              orderedVisibleOptions={props.orderedVisibleOptions}
              value={props.value}
              activeIndex={props.activeIndex}
              emptyText={props.emptyText}
              handleSelect={props.handleSelect}
              renderOption={props.renderOption}
            />
          )}
          {props.footer ? <View style={styles.footer}>{props.footer}</View> : null}
        </FloatingSurface>
      </View>
    </OverlayLayerProvider>
  );

  if (isWeb && typeof document !== "undefined") {
    return createPortal(overlay, getOverlayRoot());
  }

  return (
    <Modal transparent animationType="none" visible onRequestClose={props.handleClose}>
      {/* Android Modal opens a separate window outside the app's gesture root. */}
      <GestureHandlerRootView style={styles.desktopOverlay}>{overlay}</GestureHandlerRootView>
    </Modal>
  );
}

export function Combobox({
  options,
  value,
  onSelect,
  renderOption,
  onSearchQueryChange,
  searchable = true,
  placeholder,
  searchPlaceholder,
  emptyText,
  title,
  header,
  mobileChildrenScrollEnabled = true,
  desktopChildrenScrollEnabled = true,
  mobileChildrenContentContainerStyle,
  presentation,
  open,
  onOpenChange,
  desktopPlacement = "bottom-start",
  desktopMinWidth,
  desktopLockWidth,
  desktopFixedHeight,
  footer,
  keepOpenOnSelect = false,
  anchorRef,
  children,
}: ComboboxProps): ReactElement | null {
  const { t } = useTranslation();
  const { theme } = useUnistyles();
  const resolvedEmptyText = emptyText ?? t("common.empty.noOptionsMatchSearch");
  const resolvedTitle = title ?? t("common.actions.select");
  const isMobile = useIsCompactFormFactor();
  const floatingLayer = useOverlayLayer("floating");
  const safeAreaInsets = useSafeAreaInsets();
  const titleColor = theme.colors.foreground;
  const { height: windowHeight, width: windowWidth } = useWindowDimensions();
  const snapPoints = useMemo(() => ["60%", "90%"], []);
  const [availableSize, setAvailableSize] = useState<AvailableSize>(null);
  const [referenceWidth, setReferenceWidth] = useState<number | null>(null);
  const [referenceLeft, setReferenceLeft] = useState<number | null>(null);
  const [referenceTop, setReferenceTop] = useState<number | null>(null);
  const [referenceAtOrigin, setReferenceAtOrigin] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResetKey, bumpSearchResetKey] = useReducer((key: number) => key + 1, 0);
  const [activeIndex, setActiveIndex] = useState<number>(-1);
  const [desktopContentWidth, setDesktopContentWidth] = useState<number | null>(null);

  const [internalOpen, setInternalOpen] = useState(false);
  const isControlled = typeof open === "boolean";
  const isOpen = isControlled ? open : internalOpen;

  const setOpen = useCallback(
    (nextOpen: boolean) => {
      if (!isControlled) {
        setInternalOpen(nextOpen);
      }
      onOpenChange?.(nextOpen);
    },
    [isControlled, onOpenChange],
  );

  const setSearchQueryWithCallback = useCallback(
    (nextQuery: string) => {
      setSearchQuery(nextQuery);
      onSearchQueryChange?.(nextQuery);
    },
    [onSearchQueryChange],
  );

  const handleClose = useCallback(() => {
    setOpen(false);
    setSearchQueryWithCallback("");
    bumpSearchResetKey();
  }, [setOpen, setSearchQueryWithCallback]);

  useResetSearchOnOpen(isOpen, setSearchQueryWithCallback, bumpSearchResetKey);

  const collisionPadding = useMemo(computeCollisionPadding, []);
  const [keyboardTop, setKeyboardTop] = useState<number | null>(null);
  useEffect(() => {
    if (isWeb || isMobile || !isOpen) return;
    setKeyboardTop(Keyboard.metrics()?.screenY ?? null);
    const show = Keyboard.addListener("keyboardDidShow", (event) =>
      setKeyboardTop(event.endCoordinates.screenY),
    );
    const change = Keyboard.addListener("keyboardDidChangeFrame", (event) =>
      setKeyboardTop(event.endCoordinates.screenY),
    );
    const hide = Keyboard.addListener("keyboardDidHide", () => setKeyboardTop(null));
    return () => {
      show.remove();
      change.remove();
      hide.remove();
    };
  }, [isOpen, isMobile]);
  const floatingCollisionPadding = useMemo(
    () =>
      isWeb
        ? collisionPadding
        : nativeComboboxCollisionPadding({
            windowHeight,
            keyboardTop,
            padding: collisionPadding,
          }),
    [collisionPadding, windowHeight, keyboardTop],
  );

  const middleware = useMemo(
    () =>
      buildFloatingMiddleware({
        collisionPadding: floatingCollisionPadding,
        setAvailableSize,
        setReferenceWidth,
      }),
    [floatingCollisionPadding],
  );

  const { refs, floatingStyles, update } = useFloating({
    placement: isWeb ? desktopPlacement : "bottom-start",
    middleware,
    sameScrollView: false,
    elements: {
      reference: anchorRef.current ?? undefined,
    },
  });

  useDesktopPositionReset(isOpen, isMobile, desktopPlacement, update, {
    setAvailableSize,
    setDesktopContentWidth,
    setReferenceLeft,
    setReferenceWidth,
  });

  useAnchorMeasure(isOpen, isMobile, anchorRef, searchQuery, windowHeight, {
    setReferenceLeft,
    setReferenceTop,
    setReferenceWidth,
    setReferenceAtOrigin,
  });

  const { desktopPositionStyle, hasResolvedDesktopPosition, useMeasuredTopStartPosition } =
    computeDesktopPosition({
      isMobile,
      desktopPlacement,
      referenceTop,
      referenceLeft,
      referenceAtOrigin,
      desktopContentWidth,
      windowWidth,
      windowHeight,
      collisionPadding,
      floatingTop: toNumericStyleValue(floatingStyles.top),
      floatingLeft: toNumericStyleValue(floatingStyles.left),
      floatingStyles,
      referenceWidth,
    });

  const {
    sheetRef: bottomSheetRef,
    handleSheetChange,
    handleSheetDismiss,
  } = useIsolatedBottomSheetVisibility({
    visible: isOpen,
    isEnabled: isMobile,
    onClose: handleClose,
  });

  const normalizedSearch = searchable ? searchQuery.trim().toLowerCase() : "";
  // A fresh array on every query change re-runs the active-index sync, even
  // when the change (such as a trailing space) leaves the results unchanged.
  const orderedVisibleOptions = useMemo(
    () => [
      ...filterAndRankComboboxOptions(options, searchable ? searchQuery.trim().toLowerCase() : ""),
    ],
    [options, searchQuery, searchable],
  );

  const handleDesktopContentLayout = useCallback(
    (event: LayoutChangeEvent) => {
      const { width } = event.nativeEvent.layout;
      setDesktopContentWidth((prev) => (prev === width ? prev : width));
      if (!useMeasuredTopStartPosition || !hasResolvedDesktopPosition) {
        void update();
      }
    },
    [useMeasuredTopStartPosition, hasResolvedDesktopPosition, update],
  );

  useDesktopFloatingUpdate(
    isOpen,
    isMobile,
    orderedVisibleOptions.length,
    searchQuery,
    `${windowWidth}:${windowHeight}:${keyboardTop}`,
    update,
  );

  useActiveIndexSync(
    isOpen,
    isMobile,
    orderedVisibleOptions,
    normalizedSearch,
    value,
    setActiveIndex,
  );

  const handleSelect = useCallback(
    (id: string) => {
      onSelect(id);
      if (!keepOpenOnSelect) {
        handleClose();
      }
    },
    [handleClose, keepOpenOnSelect, onSelect],
  );

  const handleDesktopKey = useCallback(
    (key: DesktopKey, event: KeyboardEvent) =>
      dispatchDesktopKey(
        { orderedVisibleOptions, activeIndex, setActiveIndex, handleSelect, handleClose },
        key,
        event,
      ),
    [activeIndex, handleClose, handleSelect, orderedVisibleOptions],
  );

  useDismissKeyboardOnOpen(isOpen, isMobile);

  const handleIndicatorStyle = useMemo(
    () => ({ backgroundColor: theme.colors.palette.zinc[600] }),
    [theme.colors.palette.zinc],
  );

  const desktopFrameStyle = useMemo(
    () =>
      buildDesktopFrameStyle({
        desktopMinWidth,
        desktopLockWidth: Boolean(desktopLockWidth),
        referenceWidth,
        desktopFixedHeight,
        desktopPositionStyle,
        shouldHideDesktopContent: !hasResolvedDesktopPosition,
        availableHeight: availableSize?.height,
        nativePopover: !isWeb,
        availableWidth: Math.min(
          windowWidth - collisionPadding * 2,
          availableSize?.width ?? Number.POSITIVE_INFINITY,
        ),
      }),
    [
      desktopMinWidth,
      desktopLockWidth,
      referenceWidth,
      desktopFixedHeight,
      desktopPositionStyle,
      hasResolvedDesktopPosition,
      availableSize?.height,
      availableSize?.width,
      windowWidth,
      collisionPadding,
    ],
  );

  const effectiveSearchPlaceholder =
    searchPlaceholder ?? placeholder ?? t("common.placeholders.search");
  const hasChildren = Boolean(children);

  if (isMobile) {
    return (
      <MobileComboboxBody
        bottomSheetRef={bottomSheetRef}
        snapPoints={snapPoints}
        handleSheetChange={handleSheetChange}
        handleSheetDismiss={handleSheetDismiss}
        handleIndicatorStyle={handleIndicatorStyle}
        titleColor={titleColor}
        title={resolvedTitle}
        header={header}
        onClose={handleClose}
        footer={footer}
        searchable={searchable}
        hasChildren={hasChildren}
        mobileChildrenScrollEnabled={mobileChildrenScrollEnabled}
        mobileChildrenContentContainerStyle={mobileChildrenContentContainerStyle}
        presentation={presentation}
        searchResetKey={searchResetKey}
        searchPlaceholder={effectiveSearchPlaceholder}
        setSearchQueryWithCallback={setSearchQueryWithCallback}
        orderedVisibleOptions={orderedVisibleOptions}
        value={value}
        activeIndex={activeIndex}
        emptyText={resolvedEmptyText}
        handleSelect={handleSelect}
        renderOption={renderOption}
        safeAreaBottom={safeAreaInsets.bottom}
      >
        {children}
      </MobileComboboxBody>
    );
  }

  if (!isOpen) return null;

  return (
    <DesktopComboboxBody
      overlayLayer={floatingLayer}
      handleClose={handleClose}
      handleDesktopKey={handleDesktopKey}
      refs={refs}
      desktopFrameStyle={desktopFrameStyle}
      handleDesktopContentLayout={handleDesktopContentLayout}
      header={header}
      footer={footer}
      searchable={searchable}
      searchPlaceholder={effectiveSearchPlaceholder}
      setSearchQueryWithCallback={setSearchQueryWithCallback}
      orderedVisibleOptions={orderedVisibleOptions}
      value={value}
      activeIndex={activeIndex}
      emptyText={resolvedEmptyText}
      handleSelect={handleSelect}
      renderOption={renderOption}
      hasChildren={hasChildren}
      childrenScrollEnabled={desktopChildrenScrollEnabled}
    >
      {children}
    </DesktopComboboxBody>
  );
}

const styles = StyleSheet.create((theme) => ({
  mobileSheetFrame: {
    flex: 1,
    minHeight: 0,
  },
  mobileSheetBody: {
    flex: 1,
    minHeight: 0,
  },
  searchInputContainer: {
    flexDirection: "row",
    alignItems: "center",
    paddingHorizontal: theme.spacing[3],
    gap: theme.spacing[2],
    backgroundColor: theme.colors.surface1,
    borderBottomWidth: 1,
    borderBottomColor: theme.colors.border,
    ...(isWeb ? {} : { marginHorizontal: theme.spacing[1] }),
  },
  searchInput: {
    flex: 1,
    paddingVertical: theme.spacing[3],
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
  },
  comboboxItem: {
    flexDirection: "row",
    alignItems: "center",
    minHeight: 36,
    gap: theme.spacing[2],
    paddingHorizontal: theme.spacing[3],
    paddingVertical: theme.spacing[2],
    borderRadius: 0,
    ...(isWeb
      ? {}
      : {
          marginHorizontal: theme.spacing[1],
          marginBottom: theme.spacing[1],
        }),
  },
  comboboxItemHovered: {
    backgroundColor: theme.colors.surface1,
  },
  comboboxItemPressed: {
    backgroundColor: theme.colors.surface1,
  },
  comboboxItemActive: {
    backgroundColor: theme.colors.surface1,
  },
  comboboxItemDisabled: {
    opacity: 0.55,
  },
  comboboxItemTrailingSlot: {
    width: 16,
    alignItems: "center",
    justifyContent: "center",
  },
  comboboxItemTrailingContainer: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[1],
    marginLeft: "auto",
  },
  comboboxItemContent: {
    flex: 1,
    flexShrink: 1,
  },
  comboboxItemContentInline: {
    flexDirection: "row",
    alignItems: "baseline",
    gap: theme.spacing[2],
  },
  comboboxItemLeadingSlot: {
    width: 16,
    alignItems: "center",
    justifyContent: "center",
  },
  comboboxItemLabel: {
    fontSize: theme.fontSize.base,
    color: theme.colors.foreground,
    flexShrink: 0,
  },
  comboboxItemDescription: {
    fontSize: theme.fontSize.sm,
    color: theme.colors.foregroundMuted,
    flexShrink: 1,
  },
  emptyText: {
    paddingHorizontal: theme.spacing[3],
    paddingVertical: theme.spacing[2],
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.base,
  },
  footer: {
    borderTopWidth: 1,
    borderTopColor: theme.colors.border,
  },
  bottomSheetHeader: {
    paddingHorizontal: theme.spacing[6],
    paddingBottom: theme.spacing[2],
  },
  comboboxTitle: {
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
    textAlign: "left",
  },
  comboboxScrollContent: {
    paddingBottom: theme.spacing[8],
    paddingHorizontal: theme.spacing[2],
    paddingTop: theme.spacing[1],
  },
  desktopOverlay: {
    flex: 1,
  },
  desktopOverlayWeb: {
    ...StyleSheet.absoluteFillObject,
    pointerEvents: "auto" as const,
  },
  desktopBackdrop: {
    position: "absolute",
    top: 0,
    right: 0,
    bottom: 0,
    left: 0,
  },
  desktopContainer: {
    backgroundColor: theme.colors.surface0,
    borderRadius: theme.borderRadius.lg,
    borderWidth: 1,
    borderColor: theme.colors.border,
    ...theme.shadow.md,
    maxHeight: 400,
    overflow: "hidden",
  },
  desktopScroll: {
    flexShrink: 1,
    minHeight: 0,
  },
  desktopScrollContent: {
    paddingVertical: theme.spacing[1],
  },
  desktopChildrenScrollContent: {
    // No padding — custom children (e.g. model selector) control their own spacing
  },
  desktopChildrenBody: {
    flex: 1,
    minHeight: 0,
  },
}));
