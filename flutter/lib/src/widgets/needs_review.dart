import 'package:flutter/material.dart';

/// Compact summary banner listing how many ingredients need review.
///
/// Tap → [onTap] opens the review flow at the first flagged ingredient.
class NeedsReviewBanner extends StatelessWidget {
  final int count;
  final VoidCallback onTap;

  const NeedsReviewBanner({super.key, required this.count, required this.onTap});

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Container(
      margin: const EdgeInsets.only(bottom: 10),
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
      decoration: BoxDecoration(
        color: colors.secondaryContainer,
        borderRadius: BorderRadius.circular(8),
      ),
      child: InkWell(
        onTap: onTap,
        child: Row(
          children: [
            Icon(Icons.warning_amber_rounded, size: 18, color: colors.onSecondaryContainer),
            const SizedBox(width: 8),
            Expanded(
              child: Text(
                '$count ${count == 1 ? 'ingredient needs' : 'ingredients need'} review',
                style: TextStyle(
                  fontSize: 13,
                  color: colors.onSecondaryContainer,
                ),
              ),
            ),
            const SizedBox(width: 8),
            Text(
              'Review',
              style: TextStyle(
                fontSize: 13,
                fontWeight: FontWeight.w600,
                color: colors.onSecondaryContainer,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// One ingredient line in the recipe detail view.
///
/// Needs-review rows carry a warning icon, an explicit "Needs review"
/// label, and a subtle accent border — never color alone.
class IngredientBullet extends StatelessWidget {
  final String text;
  final bool checked;
  final bool needsReview;
  final VoidCallback onTap;

  const IngredientBullet({
    super.key,
    required this.text,
    required this.checked,
    required this.onTap,
    this.needsReview = false,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final base = theme.textTheme.bodyLarge;
    final style = base?.copyWith(
      decoration: checked ? TextDecoration.lineThrough : null,
      color: checked
          ? (base.color ?? Colors.black).withValues(alpha: 0.55)
          : base.color,
      height: 1.3,
    );

    final content = Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        if (needsReview) ...[
          Icon(
            Icons.warning_amber_rounded,
            size: 16,
            color: theme.colorScheme.error,
          ),
          const SizedBox(width: 4),
        ] else
          Text('•  ', style: base),
        Expanded(
          child: AnimatedDefaultTextStyle(
            duration: const Duration(milliseconds: 120),
            style: style ?? const TextStyle(),
            child: Text(text),
          ),
        ),
      ],
    );

    if (!needsReview) {
      return InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(6),
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 4),
          child: content,
        ),
      );
    }

    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(6),
      child: Container(
        margin: const EdgeInsets.symmetric(vertical: 2),
        padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 4),
        decoration: BoxDecoration(
          border: Border.all(
            color: theme.colorScheme.error.withValues(alpha: 0.4),
          ),
          borderRadius: BorderRadius.circular(6),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            content,
            Padding(
              padding: const EdgeInsets.only(left: 20, top: 1),
              child: Text(
                'Needs review',
                style: theme.textTheme.labelSmall?.copyWith(
                  color: theme.colorScheme.error,
                  fontWeight: FontWeight.w600,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
