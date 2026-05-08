import { memo } from "react";
import {
  Alert,
  Box,
  Chip,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Divider,
  List,
  ListItem,
  ListItemText,
  Stack,
  Typography,
  Button,
} from "@mui/material";
import {
  auditSourceLabel,
  researchGoalStatusLabel,
  type ResearchGoal,
  type ResearchGoalAudit,
} from "./ResearchGoalStatusPill";

export interface AuditDetailSection {
  title: string;
  items: string[];
  emptyText: string;
}

export function researchGoalAuditDetailSections(
  audit: ResearchGoalAudit | null | undefined,
): AuditDetailSection[] {
  return [
    {
      title: "缺口",
      items: audit?.missingRequirements ?? [],
      emptyText: "无未满足要求。",
    },
    {
      title: "下一步",
      items: audit?.nextActions ?? [],
      emptyText: "无额外动作。",
    },
    {
      title: "局限性",
      items: audit?.limitations ?? [],
      emptyText: "LLM 审计未报告局限性。",
    },
    {
      title: "冲突证据",
      items: audit?.conflictingEvidence ?? [],
      emptyText: "LLM 审计未报告冲突证据。",
    },
  ];
}

interface ResearchGoalAuditDetailsDialogProps {
  open: boolean;
  goal: ResearchGoal | null;
  onClose: () => void;
}

function SectionList({ section }: { section: AuditDetailSection }) {
  return (
    <Box>
      <Typography variant="subtitle2" sx={{ mb: 0.5, fontWeight: 800 }}>
        {section.title}
      </Typography>
      {section.items.length > 0 ? (
        <List dense disablePadding>
          {section.items.map((item, index) => (
            <ListItem key={`${section.title}-${index}`} disableGutters sx={{ py: 0.25 }}>
              <ListItemText
                primary={item}
                primaryTypographyProps={{ variant: "body2" }}
              />
            </ListItem>
          ))}
        </List>
      ) : (
        <Typography variant="body2" color="text.secondary">
          {section.emptyText}
        </Typography>
      )}
    </Box>
  );
}

export const ResearchGoalAuditDetailsDialog = memo(
  function ResearchGoalAuditDetailsDialog({
    open,
    goal,
    onClose,
  }: ResearchGoalAuditDetailsDialogProps) {
    const audit = goal?.lastAudit ?? null;
    const sections = researchGoalAuditDetailSections(audit);

    return (
      <Dialog open={open && Boolean(goal)} onClose={onClose} fullWidth maxWidth="md">
        <DialogTitle>科研目标审计详情</DialogTitle>
        <DialogContent>
          {!goal || !audit ? (
            <Alert severity="info">当前科研目标还没有 LLM 完成审计。</Alert>
          ) : (
            <Stack spacing={2} sx={{ pt: 0.5 }}>
              <Box>
                <Typography variant="caption" color="text.secondary">
                  当前科研目标
                </Typography>
                <Typography variant="body2" sx={{ mt: 0.25 }}>
                  {goal.objective}
                </Typography>
              </Box>

              <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
                <Chip
                  size="small"
                  label={`${researchGoalStatusLabel(goal.status)} · ${goal.currentCycle}/${goal.maxCycles}`}
                />
                <Chip size="small" label={auditSourceLabel(audit)} />
                <Chip
                  size="small"
                  color={audit.finalReportReady ? "success" : "default"}
                  label={audit.finalReportReady ? "最终报告就绪" : "最终报告未就绪"}
                />
                <Chip
                  size="small"
                  color={audit.complete ? "success" : "warning"}
                  label={audit.complete ? "审计通过" : "仍需推进"}
                />
                {audit.secondOpinion && (
                  <Chip
                    size="small"
                    color={audit.secondOpinion.agreesComplete ? "success" : "warning"}
                    label={
                      audit.secondOpinion.agreesComplete
                        ? "二次审计通过"
                        : "二次审计未通过"
                    }
                  />
                )}
              </Stack>

              <Alert severity={audit.complete ? "success" : "warning"}>
                {audit.summary || "LLM 审计未给出摘要。"}
              </Alert>

              <Box>
                <Typography variant="subtitle2" sx={{ mb: 0.5, fontWeight: 800 }}>
                  成功标准覆盖
                </Typography>
                {audit.criteria.length > 0 ? (
                  <Stack spacing={1}>
                    {audit.criteria.map((criterion, index) => (
                      <Box
                        key={criterion.criterionId || `${criterion.criterion}-${index}`}
                        sx={{
                          p: 1,
                          border: "1px solid",
                          borderColor: "divider",
                          borderRadius: 1,
                        }}
                      >
                        <Stack direction="row" spacing={1} alignItems="center">
                          <Chip
                            size="small"
                            color={criterion.covered ? "success" : "warning"}
                            label={criterion.covered ? "已覆盖" : "未覆盖"}
                          />
                          {criterion.criterionId && (
                            <Typography variant="caption" color="text.secondary">
                              {criterion.criterionId}
                            </Typography>
                          )}
                        </Stack>
                        <Typography variant="body2" sx={{ mt: 0.75, fontWeight: 700 }}>
                          {criterion.criterion}
                        </Typography>
                        {criterion.evidence && (
                          <Typography variant="body2" color="text.secondary" sx={{ mt: 0.5 }}>
                            {criterion.evidence}
                          </Typography>
                        )}
                      </Box>
                    ))}
                  </Stack>
                ) : (
                  <Typography variant="body2" color="text.secondary">
                    LLM 审计未返回逐条成功标准覆盖情况。
                  </Typography>
                )}
              </Box>

              {audit.secondOpinion && (
                <Box>
                  <Typography variant="subtitle2" sx={{ mb: 0.5, fontWeight: 800 }}>
                    二次 LLM 审计
                  </Typography>
                  <Alert severity={audit.secondOpinion.agreesComplete ? "success" : "warning"}>
                    {audit.secondOpinion.summary || "LLM 二次审计未给出摘要。"}
                  </Alert>
                  {audit.secondOpinion.blockingConcerns.length > 0 && (
                    <Box sx={{ mt: 1 }}>
                      <SectionList
                        section={{
                          title: "二次审计阻断点",
                          items: audit.secondOpinion.blockingConcerns,
                          emptyText: "无阻断点。",
                        }}
                      />
                    </Box>
                  )}
                  {audit.secondOpinion.requiredNextActions.length > 0 && (
                    <Box sx={{ mt: 1 }}>
                      <SectionList
                        section={{
                          title: "二次审计要求下一步",
                          items: audit.secondOpinion.requiredNextActions,
                          emptyText: "无额外动作。",
                        }}
                      />
                    </Box>
                  )}
                </Box>
              )}

              <Divider />

              <Stack spacing={1.5}>
                {sections.map((section) => (
                  <SectionList key={section.title} section={section} />
                ))}
              </Stack>
            </Stack>
          )}
        </DialogContent>
        <DialogActions>
          <Button onClick={onClose}>关闭</Button>
        </DialogActions>
      </Dialog>
    );
  },
);
