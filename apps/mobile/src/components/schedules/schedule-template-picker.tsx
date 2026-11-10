import { useTranslation } from "react-i18next";
import { useState, type ReactElement } from "react";
import { Pressable, Text, View, type LayoutChangeEvent } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import {
  ArrowLeft,
  ArrowRight,
  BookOpen,
  Check,
  Clock3,
  FileText,
  GitBranch,
  GitPullRequest,
  Globe,
  LayoutGrid,
  Moon,
  Plus,
  Radar,
  SlidersHorizontal,
  type LucideIcon,
} from "lucide-react-native";
import { Button } from "@/components/ui/button";
import type { FieldControlSize } from "@/components/ui/control-geometry";
import { Field, FormTextInput } from "@/components/ui/form-field";
import {
  getScheduleTemplateCadenceLabel,
  isScheduleTemplateReady,
  SCHEDULE_TEMPLATE_CATEGORIES,
  SCHEDULE_TEMPLATES,
  type ScheduleTemplate,
  type ScheduleTemplateCapability,
  type ScheduleTemplateId,
  type ScheduleTemplateParams,
} from "@/schedules/schedule-templates";

const TEMPLATE_ICONS: Record<ScheduleTemplateId, LucideIcon> = {
  "nightly-test-heal": Moon,
  "pr-gatekeeper": GitPullRequest,
  "domain-radar": Radar,
  "weekly-work-draft": FileText,
  "docs-freshness": BookOpen,
};
const CAPABILITY_ICONS: Record<ScheduleTemplateCapability, LucideIcon> = {
  code: GitBranch,
  pr: GitPullRequest,
  web: Globe,
  params: SlidersHorizontal,
};

export function ScheduleTemplatePicker({
  timezone,
  size,
  disabled,
  browsing,
  onBrowsingChange,
  onApply,
}: {
  timezone: string;
  size: FieldControlSize;
  disabled?: boolean;
  browsing: boolean;
  onBrowsingChange: (browsing: boolean) => void;
  onApply: (template: ScheduleTemplate, params: ScheduleTemplateParams) => void;
}): ReactElement {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<ScheduleTemplate | null>(null);
  const [applied, setApplied] = useState<ScheduleTemplate | null>(null);
  const [configuring, setConfiguring] = useState(false);
  const [params, setParams] = useState<ScheduleTemplateParams>({});
  const [wide, setWide] = useState(false);
  const handleLayout = (event: LayoutChangeEvent) => setWide(event.nativeEvent.layout.width >= 560);
  const applyTemplate = (template: ScheduleTemplate, values: ScheduleTemplateParams) => {
    if (disabled || !isScheduleTemplateReady(template, values)) return;
    onApply(template, values);
    setApplied(template);
    setConfiguring(false);
    onBrowsingChange(false);
  };
  const selectTemplate = (template: ScheduleTemplate) => {
    if (disabled) return;
    if (template.id !== selected?.id) setParams({});
    setSelected(template);
    if (template.params.length === 0) applyTemplate(template, {});
    else setConfiguring(true);
  };

  if (!browsing) {
    return (
      <View style={styles.editorHeader}>
        <View style={styles.headingGroup}>
          <Text style={styles.editorTitle}>{t("schedules.templates.configure")}</Text>
          <Text style={styles.subtitle}>
            {applied
              ? t("schedules.templates.applied", {
                  name: t(`schedules.templates.items.${applied.id}.name`),
                })
              : t("schedules.templates.description")}
          </Text>
        </View>
        <Button
          variant="ghost"
          size="sm"
          leftIcon={LayoutGrid}
          onPress={() => {
            setConfiguring(false);
            onBrowsingChange(true);
          }}
          disabled={disabled}
          testID="schedule-template-browse"
        >
          {t("schedules.templates.browse")}
        </Button>
      </View>
    );
  }

  if (configuring && selected) {
    const Icon = TEMPLATE_ICONS[selected.id];
    return (
      <View style={styles.parameterPage} testID="schedule-template-customize">
        <Button
          variant="ghost"
          size="sm"
          leftIcon={ArrowLeft}
          onPress={() => setConfiguring(false)}
          style={styles.backButton}
          disabled={disabled}
          testID="schedule-template-back"
        >
          {t("schedules.templates.back")}
        </Button>
        <View style={styles.parameterIntro}>
          <View style={styles.largeIcon}>
            <Icon size={24} strokeWidth={1.5} color={styles.icon.color} />
          </View>
          <Text style={styles.title}>{t(`schedules.templates.items.${selected.id}.name`)}</Text>
          <Text style={styles.parameterDescription}>
            {t(`schedules.templates.items.${selected.id}.description`)}
          </Text>
          <TemplateBadges template={selected} />
        </View>
        <View style={styles.parameterCard}>
          <Text style={styles.sectionTitle}>{t("schedules.templates.customize")}</Text>
          <Text style={styles.subtitle}>{t("schedules.templates.customizeHint")}</Text>
          {selected.params.map((key) => (
            <Field
              key={`${selected.id}:${key}`}
              label={t(`schedules.templates.items.${selected.id}.params.${key}.label`)}
            >
              <FormTextInput
                size={size}
                testID={`schedule-template-param-${key}`}
                accessibilityLabel={t(
                  `schedules.templates.items.${selected.id}.params.${key}.label`,
                )}
                initialValue={params[key] ?? ""}
                onChangeText={(value) => setParams((current) => ({ ...current, [key]: value }))}
                placeholder={t(
                  `schedules.templates.items.${selected.id}.params.${key}.placeholder`,
                )}
                editable={!disabled}
                autoFocus
              />
            </Field>
          ))}
          <Button
            variant="default"
            size="md"
            style={styles.parameterApply}
            onPress={() => applyTemplate(selected, params)}
            disabled={disabled || !isScheduleTemplateReady(selected, params)}
            trailing={<ArrowRight size={16} color={styles.buttonIcon.color} />}
            testID="schedule-template-apply"
          >
            {t("schedules.templates.apply")}
          </Button>
        </View>
      </View>
    );
  }

  return (
    <View style={styles.gallery} onLayout={handleLayout} testID="schedule-template-gallery">
      <View style={[styles.galleryHeader, !wide && styles.galleryHeaderNarrow]}>
        <View style={styles.headingGroup}>
          <Text style={styles.title}>{t("schedules.templates.heading")}</Text>
          <Text style={styles.subtitle}>{t("schedules.templates.description")}</Text>
        </View>
        <Button
          variant="outline"
          size="sm"
          leftIcon={Plus}
          onPress={() => onBrowsingChange(false)}
          disabled={disabled}
          testID="schedule-template-blank"
        >
          {t("schedules.templates.blank")}
        </Button>
      </View>
      <View style={styles.categories}>
        {SCHEDULE_TEMPLATE_CATEGORIES.map((category) => (
          <View key={category} style={styles.category}>
            <Text style={styles.categoryTitle}>
              {t(`schedules.templates.categories.${category}`)}
            </Text>
            <View style={styles.grid}>
              {SCHEDULE_TEMPLATES.filter((template) => template.category === category).map(
                (template) => (
                  <TemplateCard
                    key={template.id}
                    template={template}
                    selected={selected?.id === template.id}
                    wide={wide}
                    disabled={disabled}
                    onSelect={() => selectTemplate(template)}
                  />
                ),
              )}
            </View>
          </View>
        ))}
      </View>
      <Text style={styles.timezone}>{t("schedules.templates.timezone", { timezone })}</Text>
    </View>
  );
}

function TemplateCard({
  template,
  selected,
  wide,
  disabled,
  onSelect,
}: {
  template: ScheduleTemplate;
  selected: boolean;
  wide: boolean;
  disabled?: boolean;
  onSelect: () => void;
}): ReactElement {
  const { t } = useTranslation();
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const Icon = TEMPLATE_ICONS[template.id];
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={t(`schedules.templates.items.${template.id}.name`)}
      accessibilityHint={`${getScheduleTemplateCadenceLabel(template, t)} · ${t(`schedules.templates.items.${template.id}.description`)}`}
      accessibilityState={{ selected, disabled: Boolean(disabled) }}
      onPress={onSelect}
      onHoverIn={() => setHovered(true)}
      onHoverOut={() => setHovered(false)}
      onFocus={() => setFocused(true)}
      onBlur={() => setFocused(false)}
      disabled={disabled}
      style={({ pressed }) => [
        styles.card,
        wide ? styles.cardWide : styles.cardNarrow,
        (hovered || focused) && styles.cardHovered,
        selected && styles.cardSelected,
        pressed && styles.cardPressed,
        disabled && styles.disabled,
      ]}
      testID={`schedule-template-option-${template.id}`}
    >
      <View style={styles.cardHeader}>
        <View style={styles.cardIcon}>
          <Icon size={16} strokeWidth={1.6} color={styles.icon.color} />
        </View>
        <Text style={styles.cardTitle}>{t(`schedules.templates.items.${template.id}.name`)}</Text>
        {selected ? <Check size={15} color={styles.icon.color} /> : null}
      </View>
      <Text style={styles.cardDescription} numberOfLines={2}>
        {t(`schedules.templates.items.${template.id}.description`)}
      </Text>
      <TemplateBadges template={template} />
    </Pressable>
  );
}

function TemplateBadges({ template }: { template: ScheduleTemplate }): ReactElement {
  const { t } = useTranslation();
  return (
    <View style={styles.badges}>
      <View style={styles.badge}>
        <Clock3 size={11} color={styles.mutedIcon.color} />
        <Text style={styles.badgeText}>{getScheduleTemplateCadenceLabel(template, t)}</Text>
      </View>
      {template.capabilities.map((capability) => {
        const Icon = CAPABILITY_ICONS[capability];
        return (
          <View key={capability} style={styles.badge}>
            <Icon size={11} color={styles.mutedIcon.color} />
            <Text style={styles.badgeText}>
              {t(`schedules.templates.capabilities.${capability}`)}
            </Text>
          </View>
        );
      })}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  gallery: { gap: theme.spacing[8], paddingVertical: theme.spacing[2] },
  galleryHeader: { flexDirection: "row", alignItems: "center", gap: theme.spacing[6] },
  galleryHeaderNarrow: { flexDirection: "column", alignItems: "flex-start", gap: theme.spacing[4] },
  headingGroup: { flex: 1, minWidth: 0, gap: theme.spacing[2] },
  title: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize["3xl"],
    fontWeight: theme.fontWeight.medium,
    lineHeight: 30,
  },
  subtitle: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.sm, lineHeight: 19 },
  categories: { gap: theme.spacing[6] },
  category: { gap: theme.spacing[3] },
  categoryTitle: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.medium,
    letterSpacing: 0.4,
  },
  grid: { flexDirection: "row", flexWrap: "wrap", gap: theme.spacing[3] },
  card: {
    minHeight: 144,
    padding: theme.spacing[4],
    borderWidth: 1,
    borderColor: theme.colors.borderAccent,
    borderRadius: theme.borderRadius.xl,
    backgroundColor: theme.colors.surface1,
    gap: theme.spacing[3],
  },
  cardWide: { flexBasis: "47%", flexGrow: 1, maxWidth: "48.9%" },
  cardNarrow: { width: "100%" },
  cardHovered: {
    backgroundColor: theme.colors.surface2,
    borderColor: theme.colors.foregroundMuted,
  },
  cardSelected: { borderColor: theme.colors.accentBright },
  cardPressed: { opacity: 0.8 },
  cardHeader: { flexDirection: "row", alignItems: "center", gap: theme.spacing[2] },
  cardIcon: {
    width: 28,
    height: 28,
    borderRadius: theme.borderRadius.lg,
    backgroundColor: theme.colors.surface2,
    alignItems: "center",
    justifyContent: "center",
  },
  cardTitle: {
    flex: 1,
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
    lineHeight: 20,
  },
  cardDescription: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    lineHeight: 18,
    minHeight: 36,
  },
  badges: {
    flexDirection: "row",
    flexWrap: "wrap",
    alignItems: "center",
    gap: theme.spacing[1.5],
    marginTop: "auto",
  },
  badge: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[1],
    borderRadius: theme.borderRadius.full,
    backgroundColor: theme.colors.surface2,
    paddingVertical: theme.spacing[1],
    paddingHorizontal: theme.spacing[2],
  },
  badgeText: { color: theme.colors.foregroundMuted, fontSize: 11, lineHeight: 14 },
  timezone: { color: theme.colors.foregroundMuted, fontSize: 11, lineHeight: 16 },
  icon: { color: theme.colors.foreground },
  mutedIcon: { color: theme.colors.foregroundMuted },
  buttonIcon: { color: theme.colors.accentForeground },
  disabled: { opacity: theme.opacity[50] },
  editorHeader: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[4],
    paddingBottom: theme.spacing[2],
  },
  editorTitle: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize["2xl"],
    fontWeight: theme.fontWeight.medium,
    lineHeight: 28,
  },
  parameterPage: { gap: theme.spacing[8], paddingVertical: theme.spacing[2] },
  backButton: { alignSelf: "flex-start" },
  parameterIntro: { alignItems: "center", gap: theme.spacing[3] },
  largeIcon: {
    width: 52,
    height: 52,
    borderRadius: theme.borderRadius["2xl"],
    backgroundColor: theme.colors.surface2,
    alignItems: "center",
    justifyContent: "center",
    marginBottom: theme.spacing[2],
  },
  parameterDescription: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.base,
    lineHeight: 22,
    textAlign: "center",
  },
  parameterCard: {
    borderWidth: 1,
    borderColor: theme.colors.borderAccent,
    backgroundColor: theme.colors.surface1,
    borderRadius: theme.borderRadius.xl,
    padding: theme.spacing[6],
    gap: theme.spacing[4],
  },
  sectionTitle: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.lg,
    fontWeight: theme.fontWeight.medium,
  },
  parameterApply: { alignSelf: "flex-end", minWidth: 144 },
}));
