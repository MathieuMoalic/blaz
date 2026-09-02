import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:blaz/src/api.dart';
import 'package:blaz/src/views/edit_recipe_page.dart';
import 'package:blaz/src/widgets/needs_review.dart';
import 'package:blaz/src/widgets/toast.dart';

Recipe _recipe() {
  return Recipe(
    id: 1,
    title: 'Test Recipe',
    source: '',
    yieldText: '',
    notes: '',
    createdAt: '2026-01-01',
    updatedAt: '2026-01-01',
    ingredients: [
      Ingredient(name: '2 potatoes', foodId: 845),
      Ingredient(name: 'Juice & Zest 1 Lemon', needsReview: true),
      Ingredient(name: '1 tsp cumin', foodId: 360),
      Ingredient(name: 'pinch of mystery spice', needsReview: true),
    ],
    instructions: ['Boil'],
  );
}

Recipe _resolvedRecipe() {
  return Recipe(
    id: 1,
    title: 'Test Recipe',
    source: '',
    yieldText: '',
    notes: '',
    createdAt: '2026-01-01',
    updatedAt: '2026-01-01',
    ingredients: [
      Ingredient(name: '2 potatoes', foodId: 845),
      Ingredient(name: 'Juice & Zest 1 Lemon', foodId: 700),
      Ingredient(name: '1 tsp cumin', foodId: 360),
      Ingredient(name: 'pinch of mystery spice', foodId: 999),
    ],
    instructions: ['Boil'],
  );
}

void main() {
  group('IngredientBullet', () {
    testWidgets('needs-review rows get icon + label, normal rows do not', (
      tester,
    ) async {
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: ListView(
              children: [
                IngredientBullet(text: '2 potatoes', checked: false, onTap: () {}),
                IngredientBullet(
                  text: 'Juice & Zest 1 Lemon',
                  checked: false,
                  needsReview: true,
                  onTap: () {},
                ),
              ],
            ),
          ),
        ),
      );

      expect(find.byIcon(Icons.warning_amber_rounded), findsOneWidget);
      expect(find.text('Needs review'), findsOneWidget);
      expect(find.text('2 potatoes'), findsOneWidget);
      expect(find.text('Juice & Zest 1 Lemon'), findsOneWidget);
    });
  });

  group('NeedsReviewBanner', () {
    testWidgets('shows the count and offers the Review action', (
      tester,
    ) async {
      var tapped = false;
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: NeedsReviewBanner(
              count: 2,
              onTap: () => tapped = true,
            ),
          ),
        ),
      );

      expect(find.text('2 ingredients need review'), findsOneWidget);
      expect(find.text('Review'), findsOneWidget);
      expect(find.byIcon(Icons.warning_amber_rounded), findsOneWidget);

      await tester.tap(find.text('Review'));
      expect(tapped, isTrue);
    });

    testWidgets('uses singular wording for one ingredient', (tester) async {
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: NeedsReviewBanner(count: 1, onTap: () {}),
          ),
        ),
      );
      expect(find.text('1 ingredient needs review'), findsOneWidget);
    });
  });

  group('edit recipe needs-review rows', () {
    testWidgets('each flagged row is individually marked; count matches', (
      tester,
    ) async {
      await tester.pumpWidget(
        MaterialApp(home: EditRecipePage(recipe: _recipe())),
      );
      await tester.pump();

      expect(find.text('2 need review'), findsOneWidget);
      expect(find.text('Needs review'), findsNWidgets(2));
      expect(find.byIcon(Icons.warning_amber_rounded), findsNWidgets(2));

      // Normal rows keep the plain look (no marker on them).
      expect(find.text('2 potatoes'), findsOneWidget);
    });

    testWidgets('a fully resolved recipe shows no markers at all', (
      tester,
    ) async {
      await tester.pumpWidget(
        MaterialApp(home: EditRecipePage(recipe: _resolvedRecipe())),
      );
      await tester.pump();

      expect(find.byIcon(Icons.warning_amber_rounded), findsNothing);
      expect(find.text('Needs review'), findsNothing);
      expect(find.text('2 need review'), findsNothing);
    });
  });

  group('showShortToast', () {
    testWidgets('shows a floating, short snackbar that auto-dismisses', (
      tester,
    ) async {
      await tester.pumpWidget(
        MaterialApp(
          home: Builder(
            builder: (context) {
              return Scaffold(
                body: Center(
                  child: FilledButton(
                    onPressed: () => showShortToast(context, 'Saved'),
                    child: const Text('Do it'),
                  ),
                ),
              );
            },
          ),
        ),
      );

      await tester.tap(find.text('Do it'));
      await tester.pump();
      await tester.pump();

      final snack = find.byType(SnackBar);
      expect(snack, findsOneWidget);
      final widget = tester.widget<SnackBar>(snack);
      // Floating so it never covers the bottom navigation, and short-lived
      // so routine feedback does not linger.
      expect(widget.behavior, SnackBarBehavior.floating);
      expect(widget.duration, const Duration(milliseconds: 1500));
      expect(widget.duration.inMilliseconds, lessThanOrEqualTo(1800));
    });
  });
}
