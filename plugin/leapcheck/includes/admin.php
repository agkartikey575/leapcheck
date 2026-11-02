<?php
/**
 * Admin screen: Tools → LeapCheck.
 *
 * @package LeapCheck
 */

if ( ! defined( 'ABSPATH' ) ) {
	exit;
}

add_action( 'admin_menu', 'leapcheck_admin_menu' );
add_action( 'admin_enqueue_scripts', 'leapcheck_admin_assets' );
add_action( 'admin_post_leapcheck_consent', 'leapcheck_handle_consent' );
add_action( 'admin_post_leapcheck_revoke', 'leapcheck_handle_revoke' );
add_action( 'admin_post_leapcheck_refresh', 'leapcheck_handle_refresh' );
add_filter( 'plugin_action_links_' . plugin_basename( LEAPCHECK_FILE ), 'leapcheck_action_links' );

/**
 * Register the Tools submenu page.
 */
function leapcheck_admin_menu() {
	add_management_page(
		__( 'LeapCheck', 'leapcheck' ),
		__( 'LeapCheck', 'leapcheck' ),
		'manage_options',
		'leapcheck',
		'leapcheck_render_page'
	);
}

/**
 * URL of our admin page.
 *
 * @param array $args Extra query args.
 * @return string
 */
function leapcheck_page_url( array $args = array() ) {
	return add_query_arg( array_merge( array( 'page' => 'leapcheck' ), $args ), admin_url( 'tools.php' ) );
}

/**
 * "Check PHP compatibility" link on the Plugins screen.
 *
 * @param string[] $links Existing links.
 * @return string[]
 */
function leapcheck_action_links( $links ) {
	if ( current_user_can( 'manage_options' ) ) {
		array_unshift( $links, '<a href="' . esc_url( leapcheck_page_url() ) . '">' . esc_html__( 'Check PHP compatibility', 'leapcheck' ) . '</a>' );
	}
	return $links;
}

/**
 * Load the local stylesheet on our page only.
 *
 * @param string $hook_suffix Current admin page.
 */
function leapcheck_admin_assets( $hook_suffix ) {
	if ( 'tools_page_leapcheck' !== $hook_suffix ) {
		return;
	}
	wp_enqueue_style( 'leapcheck-admin', plugins_url( 'assets/admin.css', LEAPCHECK_FILE ), array(), LEAPCHECK_VERSION );
}

/**
 * Shared guard for form handlers: capability + nonce.
 *
 * @param string $action Nonce action.
 */
function leapcheck_verify_request( $action ) {
	if ( ! current_user_can( 'manage_options' ) ) {
		wp_die( esc_html__( 'Sorry, you are not allowed to do this.', 'leapcheck' ), '', array( 'response' => 403 ) );
	}
	check_admin_referer( $action );
}

/**
 * Opt in: store consent.
 */
function leapcheck_handle_consent() {
	leapcheck_verify_request( 'leapcheck_consent' );
	update_option(
		LEAPCHECK_OPTION_CONSENT,
		array(
			'time'    => time(),
			'user_id' => get_current_user_id(),
		),
		false
	);
	wp_safe_redirect( leapcheck_page_url( array( 'leapcheck_msg' => 'consented' ) ) );
	exit;
}

/**
 * Opt out: remove consent and all cached results.
 */
function leapcheck_handle_revoke() {
	leapcheck_verify_request( 'leapcheck_revoke' );
	delete_option( LEAPCHECK_OPTION_CONSENT );
	leapcheck_cache_clear();
	wp_safe_redirect( leapcheck_page_url( array( 'leapcheck_msg' => 'revoked' ) ) );
	exit;
}

/**
 * Clear the cache so results are fetched again.
 */
function leapcheck_handle_refresh() {
	leapcheck_verify_request( 'leapcheck_refresh' );
	leapcheck_cache_clear();
	wp_safe_redirect( leapcheck_page_url( array( 'leapcheck_msg' => 'refreshed' ) ) );
	exit;
}

/**
 * A small POST form that runs one admin-post action.
 *
 * @param string $action Action name (also the nonce action).
 * @param string $label  Button text (already translated).
 * @param string $class  Button classes.
 */
function leapcheck_action_button( $action, $label, $class = 'button' ) {
	?>
	<form method="post" action="<?php echo esc_url( admin_url( 'admin-post.php' ) ); ?>" class="leapcheck-inline-form">
		<input type="hidden" name="action" value="<?php echo esc_attr( $action ); ?>" />
		<?php wp_nonce_field( $action ); ?>
		<button type="submit" class="<?php echo esc_attr( $class ); ?>"><?php echo esc_html( $label ); ?></button>
	</form>
	<?php
}

/**
 * Icon and label for a verdict.
 *
 * @param string $verdict compatible|issues|breaks|unknown|missing|error.
 * @return string[] array( icon, label )
 */
function leapcheck_verdict_label( $verdict ) {
	switch ( $verdict ) {
		case 'compatible':
			return array( '✅', __( 'Compatible', 'leapcheck' ) );
		case 'issues':
			return array( '⚠️', __( 'Issues', 'leapcheck' ) );
		case 'breaks':
			return array( '❌', __( 'Breaks', 'leapcheck' ) );
		case 'unknown':
			return array( '❓', __( 'Could not be scanned', 'leapcheck' ) );
		case 'missing':
			return array( '❓', __( 'Not in database yet', 'leapcheck' ) );
		default:
			return array( '❓', __( 'Could not be fetched', 'leapcheck' ) );
	}
}

/**
 * Print a verdict badge.
 *
 * @param string $verdict Verdict.
 */
function leapcheck_print_verdict( $verdict ) {
	list( $icon, $label ) = leapcheck_verdict_label( $verdict );
	printf(
		'<span class="leapcheck-verdict leapcheck-verdict--%1$s"><span aria-hidden="true">%2$s</span> %3$s</span>',
		esc_attr( $verdict ),
		esc_html( $icon ),
		esc_html( $label )
	);
}

/**
 * Notice after a form action.
 */
function leapcheck_print_notice() {
	// Display-only flag set by our own redirect; no state change, so no nonce needed.
	$msg = isset( $_GET['leapcheck_msg'] ) ? sanitize_key( wp_unslash( $_GET['leapcheck_msg'] ) ) : ''; // phpcs:ignore WordPress.Security.NonceVerification.Recommended
	$messages = array(
		'consented' => __( 'Thank you. Results are now loaded from the LeapCheck database.', 'leapcheck' ),
		'revoked'   => __( 'Checking is turned off and cached results were deleted. LeapCheck will not contact the database until you opt in again.', 'leapcheck' ),
		'refreshed' => __( 'Cached results were cleared and loaded again.', 'leapcheck' ),
	);
	if ( isset( $messages[ $msg ] ) ) {
		printf( '<div class="notice notice-success is-dismissible"><p>%s</p></div>', esc_html( $messages[ $msg ] ) );
	}
}

/**
 * Render the admin page.
 */
function leapcheck_render_page() {
	if ( ! current_user_can( 'manage_options' ) ) {
		wp_die( esc_html__( 'Sorry, you are not allowed to access this page.', 'leapcheck' ) );
	}

	$items   = leapcheck_get_items();
	$consent = leapcheck_has_consent();
	$rows    = array();
	foreach ( $items as $item ) {
		$rows[] = array(
			'item'   => $item,
			'record' => $consent ? leapcheck_get_result( $item ) : array( 'status' => 'none' ),
		);
	}
	?>
	<div class="wrap leapcheck">
		<h1><?php esc_html_e( 'LeapCheck: check before you upgrade PHP', 'leapcheck' ); ?></h1>
		<?php leapcheck_print_notice(); ?>

		<p class="leapcheck-current">
			<?php
			printf(
				/* translators: %s: PHP version, e.g. 8.2.10 */
				esc_html__( 'This site currently runs PHP %s.', 'leapcheck' ),
				'<strong>' . esc_html( PHP_VERSION ) . '</strong>'
			);
			?>
		</p>

		<?php
		if ( $consent ) {
			leapcheck_render_summary( $rows );
		} else {
			leapcheck_render_consent( $items );
		}

		leapcheck_render_table( $rows, $consent );
		?>

		<p class="description">
			<?php esc_html_e( 'Results come from automated static analysis of the public plugin and theme code. Static analysis cannot see everything (for example code built at runtime), so results are a strong guide, not a guarantee. Test on a staging copy before you upgrade.', 'leapcheck' ); ?>
		</p>

		<?php if ( $consent ) : ?>
			<p class="leapcheck-actions">
				<?php leapcheck_action_button( 'leapcheck_refresh', __( 'Refresh results', 'leapcheck' ) ); ?>
				<?php leapcheck_action_button( 'leapcheck_revoke', __( 'Turn off checking and delete cached results', 'leapcheck' ), 'button button-link-delete' ); ?>
			</p>
		<?php endif; ?>
	</div>
	<?php
}

/**
 * Opt-in box shown before anything is fetched.
 *
 * @param array[] $items Inventory.
 */
function leapcheck_render_consent( array $items ) {
	$example = '';
	foreach ( $items as $item ) {
		if ( $item['valid'] ) {
			$example = leapcheck_result_url( $item );
			break;
		}
	}
	?>
	<div class="leapcheck-consent">
		<h2><?php esc_html_e( 'Check your plugins and themes against the LeapCheck database?', 'leapcheck' ); ?></h2>
		<p><?php esc_html_e( 'LeapCheck has scanned public WordPress.org plugins and themes for PHP 8.3, 8.4 and 8.5 problems. To show you the results, this site needs to download one public result file per plugin and theme from the LeapCheck server. Nothing is downloaded until you agree.', 'leapcheck' ); ?></p>
		<ul class="leapcheck-list">
			<li><?php esc_html_e( 'Only the plugin or theme folder name and its version number are used, as part of the file address.', 'leapcheck' ); ?></li>
			<li><?php esc_html_e( 'These are plain downloads of public files. No site data is sent: no site address, no user data, no content, no settings.', 'leapcheck' ); ?></li>
			<li><?php esc_html_e( 'Results are stored on this site for 12 hours. You can turn this off at any time, which also deletes the stored results.', 'leapcheck' ); ?></li>
		</ul>
		<?php if ( '' !== $example ) : ?>
			<p>
				<?php esc_html_e( 'Example of a file address that would be requested:', 'leapcheck' ); ?>
				<code><?php echo esc_html( $example ); ?></code>
			</p>
		<?php endif; ?>
		<?php leapcheck_action_button( 'leapcheck_consent', __( 'Yes, check my plugins and themes', 'leapcheck' ), 'button button-primary' ); ?>
	</div>
	<?php
}

/**
 * "Safe to upgrade to PHP X?" summary, one line per target.
 *
 * @param array[] $rows Items with records.
 */
function leapcheck_render_summary( array $rows ) {
	?>
	<div class="leapcheck-summary">
		<h2><?php esc_html_e( 'Safe to upgrade?', 'leapcheck' ); ?></h2>
		<ul class="leapcheck-summary-list">
			<?php
			foreach ( leapcheck_targets() as $target ) {
				$sum = leapcheck_summarize( $rows, $target );
				switch ( $sum['state'] ) {
					case 'breaks':
						$icon = '❌';
						/* translators: 1: PHP version, 2: number of items */
						$text = sprintf( _n( 'Safe to upgrade to PHP %1$s? Not yet: %2$d plugin or theme in use breaks.', 'Safe to upgrade to PHP %1$s? Not yet: %2$d plugins or themes in use break.', $sum['breaks'], 'leapcheck' ), $target, $sum['breaks'] );
						break;
					case 'issues':
						$icon = '⚠️';
						/* translators: 1: PHP version, 2: number of items */
						$text = sprintf( _n( 'Safe to upgrade to PHP %1$s? Probably: nothing breaks, but %2$d plugin or theme shows warnings.', 'Safe to upgrade to PHP %1$s? Probably: nothing breaks, but %2$d plugins or themes show warnings.', $sum['issues'], 'leapcheck' ), $target, $sum['issues'] );
						break;
					case 'unchecked':
						$icon = '❓';
						/* translators: 1: PHP version */
						$text = sprintf( __( 'Safe to upgrade to PHP %1$s? No known problems, but not everything could be checked.', 'leapcheck' ), $target );
						break;
					case 'compatible':
						$icon = '✅';
						/* translators: 1: PHP version */
						$text = sprintf( __( 'Safe to upgrade to PHP %1$s? Yes: all plugins and themes in use are compatible.', 'leapcheck' ), $target );
						break;
					default:
						$icon = '❓';
						/* translators: 1: PHP version */
						$text = sprintf( __( 'Safe to upgrade to PHP %1$s? No plugins or themes in use to check.', 'leapcheck' ), $target );
				}
				?>
				<li class="leapcheck-summary--<?php echo esc_attr( $sum['state'] ); ?>">
					<span aria-hidden="true"><?php echo esc_html( $icon ); ?></span>
					<strong><?php echo esc_html( $text ); ?></strong>
					<?php if ( $sum['unchecked'] > 0 && 'unchecked' !== $sum['state'] ) : ?>
						<?php
						/* translators: %d: number of items */
						echo esc_html( sprintf( _n( '(%d item could not be checked.)', '(%d items could not be checked.)', $sum['unchecked'], 'leapcheck' ), $sum['unchecked'] ) );
						?>
					<?php elseif ( 'unchecked' === $sum['state'] ) : ?>
						<?php
						/* translators: %d: number of items */
						echo esc_html( sprintf( _n( '(%d item is not in the database yet or could not be fetched.)', '(%d items are not in the database yet or could not be fetched.)', $sum['unchecked'], 'leapcheck' ), $sum['unchecked'] ) );
						?>
					<?php endif; ?>
					<?php if ( version_compare( PHP_VERSION, $target, '>=' ) ) : ?>
						<em><?php esc_html_e( '(This site already runs this version or newer.)', 'leapcheck' ); ?></em>
					<?php endif; ?>
				</li>
				<?php
			}
			?>
		</ul>
		<p class="description"><?php esc_html_e( 'The summary counts active plugins and the active theme (and its parent). Inactive plugins are listed below but not counted.', 'leapcheck' ); ?></p>
	</div>
	<?php
}

/**
 * Inventory / results table.
 *
 * @param array[] $rows    Items with records.
 * @param bool    $consent Whether results were loaded.
 */
function leapcheck_render_table( array $rows, $consent ) {
	$targets = leapcheck_targets();
	$columns = 4 + ( $consent ? count( $targets ) : 0 );
	?>
	<h2><?php esc_html_e( 'Installed plugins and themes', 'leapcheck' ); ?></h2>
	<table class="widefat striped leapcheck-table">
		<thead>
			<tr>
				<th scope="col"><?php esc_html_e( 'Name', 'leapcheck' ); ?></th>
				<th scope="col"><?php esc_html_e( 'Type', 'leapcheck' ); ?></th>
				<th scope="col"><?php esc_html_e( 'Version', 'leapcheck' ); ?></th>
				<th scope="col"><?php esc_html_e( 'Status', 'leapcheck' ); ?></th>
				<?php if ( $consent ) : ?>
					<?php foreach ( $targets as $target ) : ?>
						<th scope="col">
							<?php
							/* translators: %s: PHP version */
							echo esc_html( sprintf( __( 'PHP %s', 'leapcheck' ), $target ) );
							?>
						</th>
					<?php endforeach; ?>
				<?php endif; ?>
			</tr>
		</thead>
		<tbody>
			<?php foreach ( $rows as $row ) : ?>
				<?php
				$item   = $row['item'];
				$record = $row['record'];
				if ( 'active-theme' === $item['role'] ) {
					$type = __( 'Active theme', 'leapcheck' );
				} elseif ( 'parent-theme' === $item['role'] ) {
					$type = __( 'Parent theme', 'leapcheck' );
				} else {
					$type = __( 'Plugin', 'leapcheck' );
				}
				?>
				<tr>
					<td><strong><?php echo esc_html( $item['name'] ); ?></strong><br /><code><?php echo esc_html( $item['slug'] ); ?></code></td>
					<td><?php echo esc_html( $type ); ?></td>
					<td><?php echo esc_html( '' !== $item['version'] ? $item['version'] : '—' ); ?></td>
					<td><?php echo $item['active'] ? esc_html__( 'Active', 'leapcheck' ) : esc_html__( 'Inactive', 'leapcheck' ); ?></td>
					<?php if ( $consent ) : ?>
						<?php foreach ( $targets as $target ) : ?>
							<td><?php leapcheck_print_verdict( leapcheck_item_verdict( $record, $target ) ); ?></td>
						<?php endforeach; ?>
					<?php endif; ?>
				</tr>
				<?php if ( $consent && isset( $record['status'] ) && 'ok' === $record['status'] ) : ?>
					<tr class="leapcheck-details-row">
						<td colspan="<?php echo esc_attr( (string) $columns ); ?>">
							<?php leapcheck_render_details( $record['data'] ); ?>
						</td>
					</tr>
				<?php endif; ?>
			<?php endforeach; ?>
		</tbody>
	</table>
	<?php
}

/**
 * Expandable findings and bundled libraries for one item.
 *
 * @param array $data Parsed result.
 */
function leapcheck_render_details( array $data ) {
	$finding_count = 0;
	foreach ( $data['targets'] as $target ) {
		$finding_count += $target['findings_total'];
	}
	$component_count = count( $data['components'] );
	?>
	<details class="leapcheck-details">
		<summary>
			<?php
			echo esc_html(
				sprintf(
					/* translators: 1: number of findings, 2: number of bundled libraries */
					__( 'Details: %1$d findings, %2$d bundled libraries', 'leapcheck' ),
					$finding_count,
					$component_count
				)
			);
			?>
		</summary>

		<?php foreach ( leapcheck_targets() as $target ) : ?>
			<?php
			$key = 'php:' . $target;
			if ( empty( $data['targets'][ $key ]['findings'] ) ) {
				continue;
			}
			$t = $data['targets'][ $key ];
			?>
			<h4>
				<?php
				/* translators: 1: PHP version, 2: number of errors, 3: number of warnings */
				echo esc_html( sprintf( __( 'PHP %1$s: %2$d errors, %3$d warnings', 'leapcheck' ), $target, $t['errors'], $t['warnings'] ) );
				?>
			</h4>
			<table class="widefat leapcheck-findings">
				<thead>
					<tr>
						<th scope="col"><?php esc_html_e( 'Severity', 'leapcheck' ); ?></th>
						<th scope="col"><?php esc_html_e( 'File', 'leapcheck' ); ?></th>
						<th scope="col"><?php esc_html_e( 'Line', 'leapcheck' ); ?></th>
						<th scope="col"><?php esc_html_e( 'Message', 'leapcheck' ); ?></th>
					</tr>
				</thead>
				<tbody>
					<?php foreach ( $t['findings'] as $f ) : ?>
						<tr>
							<td><?php echo 'error' === $f['severity'] ? esc_html__( 'Error', 'leapcheck' ) : esc_html__( 'Warning', 'leapcheck' ); ?></td>
							<td>
								<code><?php echo esc_html( $f['file'] ); ?></code>
								<?php if ( '' !== $f['component'] ) : ?>
									<br />
									<?php
									/* translators: %s: bundled library name */
									echo esc_html( sprintf( __( 'in bundled library %s', 'leapcheck' ), $f['component'] ) );
									?>
								<?php endif; ?>
							</td>
							<td><?php echo $f['line'] > 0 ? esc_html( (string) $f['line'] ) : '—'; ?></td>
							<td><?php echo esc_html( $f['message'] ); ?></td>
						</tr>
					<?php endforeach; ?>
				</tbody>
			</table>
			<?php if ( $t['findings_total'] > count( $t['findings'] ) ) : ?>
				<p class="description">
					<?php
					/* translators: %d: number of findings not shown */
					echo esc_html( sprintf( __( '%d more findings are not shown.', 'leapcheck' ), $t['findings_total'] - count( $t['findings'] ) ) );
					?>
				</p>
			<?php endif; ?>
		<?php endforeach; ?>

		<h4><?php esc_html_e( 'Bundled libraries found inside', 'leapcheck' ); ?></h4>
		<?php if ( 0 === $component_count ) : ?>
			<p><?php esc_html_e( 'None found.', 'leapcheck' ); ?></p>
		<?php else : ?>
			<table class="widefat leapcheck-components">
				<thead>
					<tr>
						<th scope="col"><?php esc_html_e( 'Library', 'leapcheck' ); ?></th>
						<th scope="col"><?php esc_html_e( 'Version', 'leapcheck' ); ?></th>
						<th scope="col"><?php esc_html_e( 'Location', 'leapcheck' ); ?></th>
						<th scope="col"><?php esc_html_e( 'Known vulnerabilities', 'leapcheck' ); ?></th>
					</tr>
				</thead>
				<tbody>
					<?php foreach ( $data['components'] as $c ) : ?>
						<tr>
							<td><?php echo esc_html( $c['name'] ); ?></td>
							<td><?php echo esc_html( '' !== $c['version'] ? $c['version'] : '—' ); ?></td>
							<td><code><?php echo esc_html( $c['path'] ); ?></code></td>
							<td><?php echo esc_html( (string) $c['known_vulnerabilities'] ); ?></td>
						</tr>
					<?php endforeach; ?>
				</tbody>
			</table>
		<?php endif; ?>

		<?php if ( '' !== $data['scanned_at'] ) : ?>
			<p class="description">
				<?php
				/* translators: %s: date and time of the scan */
				echo esc_html( sprintf( __( 'Scanned: %s', 'leapcheck' ), $data['scanned_at'] ) );
				?>
			</p>
		<?php endif; ?>
	</details>
	<?php
}
