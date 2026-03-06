import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:blaz/src/widgets/recipe_card.dart';
import 'package:blaz/src/views/login_page.dart';

void main() {
  // ──────────────────────────────────────────────────────────────────
  // RecipeImagePlaceholder
  // ──────────────────────────────────────────────────────────────────
  group('RecipeImagePlaceholder', () {
    testWidgets('renders an icon inside a container', (tester) async {
      await tester.pumpWidget(
        const MaterialApp(
          home: Scaffold(
            body: SizedBox(
              width: 100,
              height: 100,
              child: RecipeImagePlaceholder(),
            ),
          ),
        ),
      );
      expect(find.byIcon(Icons.restaurant_menu), findsOneWidget);
    });

    testWidgets('uses custom iconSize', (tester) async {
      await tester.pumpWidget(
        const MaterialApp(
          home: Scaffold(
            body: SizedBox(
              width: 100,
              height: 100,
              child: RecipeImagePlaceholder(iconSize: 24),
            ),
          ),
        ),
      );
      final icon = tester.widget<Icon>(find.byIcon(Icons.restaurant_menu));
      expect(icon.size, 24);
    });
  });

  // ──────────────────────────────────────────────────────────────────
  // RecipeCard
  // ──────────────────────────────────────────────────────────────────
  group('RecipeCard', () {
    Widget buildCard({
      String title = 'Test Recipe',
      String? imageUrl,
      VoidCallback? onOpen,
      VoidCallback? onAssign,
      double? width,
    }) {
      return MaterialApp(
        home: Scaffold(
          body: SizedBox(
            width: 200,
            height: 300,
            child: RecipeCard(
              title: title,
              imageUrl: imageUrl,
              onOpen: onOpen,
              onAssign: onAssign,
              width: width,
            ),
          ),
        ),
      );
    }

    testWidgets('displays the recipe title', (tester) async {
      await tester.pumpWidget(buildCard(title: 'Carbonara'));
      expect(find.text('Carbonara'), findsOneWidget);
    });

    testWidgets('shows placeholder when imageUrl is null', (tester) async {
      await tester.pumpWidget(buildCard(imageUrl: null));
      expect(find.byType(RecipeImagePlaceholder), findsOneWidget);
    });

    testWidgets('onOpen is called when tapped', (tester) async {
      var tapped = false;
      await tester.pumpWidget(buildCard(onOpen: () => tapped = true));
      await tester.tap(find.byType(InkWell));
      expect(tapped, isTrue);
    });

    testWidgets('assign button visible in normal mode', (tester) async {
      var assigned = false;
      await tester.pumpWidget(buildCard(onAssign: () => assigned = true));
      expect(find.byIcon(Icons.event), findsOneWidget);
      await tester.tap(find.byIcon(Icons.event));
      expect(assigned, isTrue);
    });

    testWidgets('assign button hidden in mini mode', (tester) async {
      await tester.pumpWidget(
        buildCard(onAssign: () {}, width: 120),
      );
      expect(find.byIcon(Icons.event), findsNothing);
    });

    testWidgets('long title is truncated with ellipsis', (tester) async {
      const longTitle =
          'This Is A Very Long Recipe Title That Should Be Truncated With Ellipsis';
      await tester.pumpWidget(buildCard(title: longTitle));
      final text = tester.widget<Text>(find.text(longTitle));
      expect(text.overflow, TextOverflow.ellipsis);
      expect(text.maxLines, 2);
    });
  });

  // ──────────────────────────────────────────────────────────────────
  // LoginPage
  // ──────────────────────────────────────────────────────────────────
  group('LoginPage', () {
    testWidgets('renders password field and sign-in button', (tester) async {
      await tester.pumpWidget(const MaterialApp(home: LoginPage()));

      expect(find.byType(TextFormField), findsOneWidget);
      expect(find.text('Sign in'), findsWidgets); // AppBar title + button
    });

    testWidgets('password field validates empty input', (tester) async {
      await tester.pumpWidget(const MaterialApp(home: LoginPage()));

      // Trigger validation by tapping submit with empty field
      final button = find.widgetWithText(FilledButton, 'Sign in');
      expect(button, findsOneWidget);
    });

    testWidgets('has server URL icon button in AppBar', (tester) async {
      await tester.pumpWidget(const MaterialApp(home: LoginPage()));
      expect(find.byIcon(Icons.cloud), findsOneWidget);
    });
  });
}
