<?php
/**
 * Site-independent test of the logic in includes/functions.php.
 * Run: php plugin/tests/test-functions.php   (exit code 0 = pass)
 *
 * WordPress functions are replaced by small in-memory stubs; HTTP is a fake URL => response map.
 *
 * @package LeapCheck
 */

define( 'ABSPATH', __DIR__ . '/' );
define( 'LEAPCHECK_VERSION', '0.2.0' );
define( 'LEAPCHECK_BASE_URL', 'https://leapcheck.vercel.app' );
define( 'MINUTE_IN_SECONDS', 60 );
define( 'HOUR_IN_SECONDS', 3600 );

// ---- WordPress stubs -------------------------------------------------------
$GLOBALS['lc_options']    = array();
$GLOBALS['lc_transients'] = array();
$GLOBALS['lc_http']       = array(); // url => array( code ) | array( code, body ) | 'neterror'.
$GLOBALS['lc_requests']   = array();

function apply_filters( $hook, $value ) {
	return $value;
}
function esc_url_raw( $url ) {
	return filter_var( $url, FILTER_VALIDATE_URL ) ? $url : '';
}
function untrailingslashit( $value ) {
	return rtrim( $value, '/\\' );
}
function get_option( $name, $default = false ) {
	return array_key_exists( $name, $GLOBALS['lc_options'] ) ? $GLOBALS['lc_options'][ $name ] : $default;
}
function update_option( $name, $value ) {
	$GLOBALS['lc_options'][ $name ] = $value;
	return true;
}
function delete_option( $name ) {
	unset( $GLOBALS['lc_options'][ $name ] );
	return true;
}
function get_transient( $key ) {
	return isset( $GLOBALS['lc_transients'][ $key ] ) ? $GLOBALS['lc_transients'][ $key ] : false;
}
function set_transient( $key, $value ) {
	$GLOBALS['lc_transients'][ $key ] = $value;
	return true;
}
function delete_transient( $key ) {
	unset( $GLOBALS['lc_transients'][ $key ] );
	return true;
}
class WP_Error {
}
function is_wp_error( $thing ) {
	return $thing instanceof WP_Error;
}
function wp_remote_get( $url, $args ) {
	$GLOBALS['lc_requests'][] = array( $url, $args );
	$fake = isset( $GLOBALS['lc_http'][ $url ] ) ? $GLOBALS['lc_http'][ $url ] : array( 404 );
	if ( 'neterror' === $fake ) {
		return new WP_Error();
	}
	return array(
		'code' => $fake[0],
		'body' => isset( $fake[1] ) ? $fake[1] : '',
	);
}
function wp_remote_retrieve_response_code( $response ) {
	return $response['code'];
}
function wp_remote_retrieve_body( $response ) {
	return $response['body'];
}

require dirname( __DIR__ ) . '/leapcheck/includes/functions.php';

$failures = 0;
function check( $label, $cond ) {
	global $failures;
	echo ( $cond ? 'ok   ' : 'FAIL ' ) . $label . "\n";
	if ( ! $cond ) {
		++$failures;
	}
}

/** Start a fresh "page load" (the per-request fetch counter resets; caches stay). */
function new_page_load() {
	leapcheck_fetch_state( 'reset' );
	$GLOBALS['lc_requests'] = array();
}

/** Start from an empty site. */
function reset_site( $consent = true ) {
	$GLOBALS['lc_options']    = $consent ? array( LEAPCHECK_OPTION_CONSENT => array( 'time' => 1 ) ) : array();
	$GLOBALS['lc_transients'] = array();
	$GLOBALS['lc_http']       = array();
	new_page_load();
}

/** A minimal valid result file. */
function result_json( $name, $version, $verdict84, array $components = array(), array $requirements = array() ) {
	return json_encode(
		array(
			'schema'       => 1,
			'ecosystem'    => 'wordpress-plugin',
			'name'         => $name,
			'version'      => $version,
			'targets'      => array(
				'php:8.4' => array(
					'verdict'  => $verdict84,
					'errors'   => 0,
					'warnings' => 0,
					'findings' => array(),
				),
			),
			'components'   => $components,
			'requirements' => $requirements,
		)
	);
}

/** A loaded row for summary / conflict tests. */
function row( $slug, $active, array $components, array $requirements = array(), $fallback = '' ) {
	$data = leapcheck_parse_result( json_decode( result_json( $slug, '1.0', 'compatible', $components, $requirements ), true ) );
	return array(
		'item'   => leapcheck_make_item( 'plugin', $slug, ucfirst( $slug ), '1.0', $active ),
		'record' => array(
			'status'   => 'ok',
			'data'     => $data,
			'fetched'  => 1,
			'fallback' => $fallback,
		),
	);
}

function lib( $name, $version, $kind = 'php' ) {
	return array(
		'name'    => $name,
		'version' => $version,
		'kind'    => $kind,
		'path'    => 'vendor/' . $name,
	);
}

// ---- Parsing the docs/07 fixture --------------------------------------------
$raw    = json_decode( file_get_contents( __DIR__ . '/sample-result.json' ), true );
$parsed = leapcheck_parse_result( $raw );
check( 'fixture parses', is_array( $parsed ) );
check( 'three targets', 3 === count( $parsed['targets'] ) );
check( 'php:8.4 has 2 counted findings (info kept apart)', 2 === count( $parsed['targets']['php:8.4']['findings'] ) && 2 === $parsed['targets']['php:8.4']['findings_total'] );
check( 'php:8.4 has 1 guarded finding', 1 === count( $parsed['targets']['php:8.4']['guarded'] ) && 1 === $parsed['targets']['php:8.4']['guarded_total'] );
check( 'guarded finding is info', 'info' === $parsed['targets']['php:8.4']['guarded'][0]['severity'] );
check( 'no info in counted findings', ! in_array( 'info', array_column( $parsed['targets']['php:8.4']['findings'], 'severity' ), true ) );
check( 'finding line is int', 161 === $parsed['targets']['php:8.4']['findings'][0]['line'] );
check( 'null component becomes empty string', '' === $parsed['targets']['php:8.4']['findings'][0]['component'] );
check( 'component parsed', 'phpmailer' === $parsed['components'][0]['name'] && '5.2.7' === $parsed['components'][0]['version'] );
check( 'requirements: unique, sorted, invalid names dropped', array( 'gd', 'intl' ) === $parsed['requirements'] );
check( 'display_name parsed', 'Contact Form 7' === $parsed['display_name'] );
check( 'active_installs parsed', 10000000 === $parsed['active_installs'] );
$no_req = leapcheck_parse_result( array( 'schema' => 1, 'targets' => array() ) );
check( 'missing requirements become empty list', array() === $no_req['requirements'] );

// Invalid input.
check( 'rejects wrong schema', null === leapcheck_parse_result( array( 'schema' => 2, 'targets' => array() ) ) );
check( 'rejects non-array', null === leapcheck_parse_result( 'nope' ) );
$odd = leapcheck_parse_result( array( 'schema' => 1, 'targets' => array( 'php:8.4' => array( 'verdict' => '<script>' ) ) ) );
check( 'unknown verdict string becomes unknown', 'unknown' === $odd['targets']['php:8.4']['verdict'] );

// ---- Verdicts ----------------------------------------------------------------
$ok = array( 'status' => 'ok', 'data' => $parsed, 'fallback' => '' );
check( '8.3 compatible', 'compatible' === leapcheck_item_verdict( $ok, '8.3' ) );
check( '8.4 issues', 'issues' === leapcheck_item_verdict( $ok, '8.4' ) );
check( '8.5 breaks', 'breaks' === leapcheck_item_verdict( $ok, '8.5' ) );
check( 'missing target is unknown', 'unknown' === leapcheck_item_verdict( $ok, '9.0' ) );
check( '404 is missing', 'missing' === leapcheck_item_verdict( array( 'status' => 'missing' ), '8.4' ) );
check( 'over fetch limit is pending', 'pending' === leapcheck_item_verdict( array( 'status' => 'pending' ), '8.4' ) );
check( 'fetch error is error', 'error' === leapcheck_item_verdict( array( 'status' => 'error' ), '8.4' ) );

// ---- Items and URLs ------------------------------------------------------------
$item = leapcheck_make_item( 'plugin', 'contact-form-7', 'Contact Form 7', '5.1', true );
check( 'plugin url', 'https://leapcheck.vercel.app/v1/wordpress-plugin/contact-form-7/5.1.json' === leapcheck_result_url( $item ) );
check( 'plugin url for other version', 'https://leapcheck.vercel.app/v1/wordpress-plugin/contact-form-7/5.0.json' === leapcheck_result_url( $item, '5.0' ) );
check( 'index url', 'https://leapcheck.vercel.app/v1/wordpress-plugin/contact-form-7/index.json' === leapcheck_index_url( $item ) );
$theme = leapcheck_make_item( 'theme', 'TwentyTwentyFour', 'Twenty Twenty-Four', '1.2', true, 'active-theme' );
check( 'theme url, slug lowercased', 'https://leapcheck.vercel.app/v1/wordpress-theme/twentytwentyfour/1.2.json' === leapcheck_result_url( $theme ) );
check( 'path traversal rejected', false === leapcheck_make_item( 'plugin', '../x', 'X', '1.0', true )['valid'] );
check( 'empty version rejected', false === leapcheck_make_item( 'plugin', 'x', 'X', '', true )['valid'] );

// ---- Index parsing and closest version -----------------------------------------
$idx = leapcheck_parse_index( array( 'schema' => 1, 'versions' => array( '1.0', '../evil', 2, '1.0', '2.0' ), 'latest' => '2.0' ) );
check( 'index: invalid and duplicate versions dropped', array( '1.0', '2.0' ) === $idx['versions'] && '2.0' === $idx['latest'] );
check( 'index: wrong schema rejected', null === leapcheck_parse_index( array( 'schema' => 2, 'versions' => array() ) ) );
check( 'index: no versions rejected', null === leapcheck_parse_index( array( 'schema' => 1 ) ) );
check( 'closest: same minor beats other major', '5.0.1' === leapcheck_closest_version( array( '4.9', '5.0.1', '6.0' ), '5.0' ) );
check( 'closest: older wins a tie', '5.1.1' === leapcheck_closest_version( array( '5.1.1', '5.1.3' ), '5.1.2' ) );
check( 'closest: nearest older of several', '5.1.1' === leapcheck_closest_version( array( '5.1.0', '5.1.1', '5.1.3' ), '5.1.2' ) );
check( 'closest: nearest newer when nothing older', '2.1' === leapcheck_closest_version( array( '3.0', '2.1', '2.5' ), '2.0' ) );
check( 'closest: none', '' === leapcheck_closest_version( array(), '1.0' ) );
check( 'closest: ignores installed version itself', '' === leapcheck_closest_version( array( '1.0' ), '1.0' ) );

// ---- Fetching: exact hit, fallback, missing ------------------------------------
$base = 'https://leapcheck.vercel.app/v1/wordpress-plugin/';
reset_site();
$GLOBALS['lc_http'][ $base . 'contact-form-7/5.1.json' ] = array( 200, file_get_contents( __DIR__ . '/sample-result.json' ) );
$r = leapcheck_get_result( $item );
check( 'exact hit: ok, no fallback', 'ok' === $r['status'] && '' === $r['fallback'] && 1 === count( $GLOBALS['lc_requests'] ) );
check( 'request sends neutral user agent only', 'LeapCheck/0.2.0' === $GLOBALS['lc_requests'][0][1]['user-agent'] );
new_page_load();
leapcheck_get_result( $item );
check( 'exact hit cached: no new request', 0 === count( $GLOBALS['lc_requests'] ) );

reset_site();
$old = leapcheck_make_item( 'plugin', 'slider', 'Slider', '3.0.2', true );
$GLOBALS['lc_http'][ $base . 'slider/index.json' ] = array( 200, json_encode( array( 'schema' => 1, 'versions' => array( '2.9', '3.0.1', '3.2' ), 'latest' => '3.2' ) ) );
$GLOBALS['lc_http'][ $base . 'slider/3.0.1.json' ] = array( 200, result_json( 'slider', '3.0.1', 'breaks' ) );
$r = leapcheck_get_result( $old );
check( 'fallback: ok with closest version', 'ok' === $r['status'] && '3.0.1' === $r['fallback'] );
check( 'fallback: verdict of closest version shown', 'breaks' === leapcheck_item_verdict( $r, '8.4' ) );
check( 'fallback: three requests (version, index, closest)', 3 === count( $GLOBALS['lc_requests'] ) );
new_page_load();
$r = leapcheck_get_result( $old );
check( 'fallback cached: no new request, still labelled', 0 === count( $GLOBALS['lc_requests'] ) && '3.0.1' === $r['fallback'] );

reset_site();
$custom = leapcheck_make_item( 'plugin', 'my-custom', 'My custom', '0.3', true );
$r      = leapcheck_get_result( $custom );
check( 'no index: missing', 'missing' === $r['status'] && null === $r['data'] && 2 === count( $GLOBALS['lc_requests'] ) );

reset_site();
$GLOBALS['lc_http'][ $base . 'my-custom/index.json' ] = array( 200, json_encode( array( 'schema' => 1, 'versions' => array( '0.3' ) ) ) );
check( 'index without another version: missing', 'missing' === leapcheck_get_result( $custom )['status'] );

reset_site();
$GLOBALS['lc_http'][ $base . 'contact-form-7/5.1.json' ] = array( 500 );
check( 'server error: error', 'error' === leapcheck_get_result( $item )['status'] );

reset_site();
$GLOBALS['lc_http'][ $base . 'contact-form-7/5.1.json' ] = array( 200, '{not json' );
check( 'damaged file: error', 'error' === leapcheck_get_result( $item )['status'] );

reset_site();
$GLOBALS['lc_http'][ $base . 'contact-form-7/5.1.json' ] = 'neterror';
check( 'network error: error', 'error' === leapcheck_get_result( $item )['status'] );
check( 'after a network error, later items are not fetched', 'error' === leapcheck_get_result( $old )['status'] && 1 === count( $GLOBALS['lc_requests'] ) );

reset_site( false );
check( 'no consent: error and no request', 'error' === leapcheck_get_result( $item )['status'] && 0 === count( $GLOBALS['lc_requests'] ) );

reset_site();
check( 'invalid item: missing and no request', 'missing' === leapcheck_get_result( leapcheck_make_item( 'plugin', '../x', 'X', '1', true ) )['status'] && 0 === count( $GLOBALS['lc_requests'] ) );

// ---- Fetch limit: at most 25 per page load ---------------------------------------
reset_site();
$many = array();
for ( $i = 1; $i <= 30; $i++ ) {
	$many[]                                           = leapcheck_make_item( 'plugin', 'p' . $i, 'P' . $i, '1.0', true );
	$GLOBALS['lc_http'][ $base . 'p' . $i . '/1.0.json' ] = array( 200, result_json( 'p' . $i, '1.0', 'compatible' ) );
}
$statuses = array();
foreach ( $many as $m ) {
	$statuses[] = leapcheck_get_result( $m )['status'];
}
$counts = array_count_values( $statuses );
check( 'limit: 25 fetched, 5 pending', 25 === $counts['ok'] && 5 === $counts['pending'] && LEAPCHECK_MAX_FETCHES === count( $GLOBALS['lc_requests'] ) );
new_page_load();
$statuses = array();
foreach ( $many as $m ) {
	$statuses[] = leapcheck_get_result( $m )['status'];
}
check( 'limit: next page load fetches only the remaining 5', array( 'ok' ) === array_values( array_unique( $statuses ) ) && 5 === count( $GLOBALS['lc_requests'] ) );

reset_site();
// 24 cheap items, then one that needs the fallback chain (3 requests): it stops at the limit as pending.
for ( $i = 1; $i <= 24; $i++ ) {
	$GLOBALS['lc_http'][ $base . 'p' . $i . '/1.0.json' ] = array( 200, result_json( 'p' . $i, '1.0', 'compatible' ) );
	leapcheck_get_result( $many[ $i - 1 ] );
}
$GLOBALS['lc_http'][ $base . 'slider/index.json' ] = array( 200, json_encode( array( 'schema' => 1, 'versions' => array( '3.0.1' ) ) ) );
$GLOBALS['lc_http'][ $base . 'slider/3.0.1.json' ] = array( 200, result_json( 'slider', '3.0.1', 'compatible' ) );
check( 'limit reached inside the fallback chain: pending', 'pending' === leapcheck_get_result( $old )['status'] );
new_page_load();
$r = leapcheck_get_result( $old );
check( 'fallback chain resumes from cache next load', 'ok' === $r['status'] && '3.0.1' === $r['fallback'] && 2 === count( $GLOBALS['lc_requests'] ) );

// ---- Cache clear -----------------------------------------------------------------
leapcheck_cache_clear();
check( 'cache clear removes every transient and the key list', array() === $GLOBALS['lc_transients'] && false === get_option( LEAPCHECK_OPTION_CACHE_KEYS ) );

// ---- Summary -----------------------------------------------------------------------
$rows = array(
	array( 'item' => $item, 'record' => $ok ),
	array( 'item' => leapcheck_make_item( 'plugin', 'custom', 'Custom', '1.0', true ), 'record' => array( 'status' => 'missing' ) ),
	array( 'item' => leapcheck_make_item( 'plugin', 'old', 'Old', '1.0', false ), 'record' => array( 'status' => 'missing' ) ),
);
$s83 = leapcheck_summarize( $rows, '8.3' );
check( 'summary 8.3 unchecked (one missing)', 'unchecked' === $s83['state'] && 2 === $s83['total'] && 1 === $s83['unchecked'] );
check( 'summary 8.4: unchecked ranks above issues', 'unchecked' === leapcheck_summarize( $rows, '8.4' )['state'] );
check( 'summary 8.4 issues', 'issues' === leapcheck_summarize( array( $rows[0] ), '8.4' )['state'] );
check( 'summary 8.5 breaks', 'breaks' === leapcheck_summarize( $rows, '8.5' )['state'] );
check( 'summary all compatible', 'compatible' === leapcheck_summarize( array( $rows[0] ), '8.3' )['state'] );
check( 'summary empty', 'empty' === leapcheck_summarize( array( $rows[2] ), '8.3' )['state'] );
$approx = row( 'approx', true, array(), array(), '0.9' );
$sa     = leapcheck_summarize( array( $rows[0], $approx ), '8.3' );
check( 'summary: closest-version result counts as unchecked, never green', 'unchecked' === $sa['state'] && 1 === $sa['approx'] );
$sp = leapcheck_summarize( array( $rows[0], array( 'item' => $many[0], 'record' => array( 'status' => 'pending' ) ) ), '8.3' );
check( 'summary: pending counts as unchecked', 'unchecked' === $sp['state'] && 1 === $sp['unchecked'] );
$guarded_only = leapcheck_parse_result(
	array(
		'schema'  => 1,
		'targets' => array(
			'php:8.4' => array(
				'verdict'  => 'compatible',
				'findings' => array( array( 'severity' => 'info', 'message' => 'guarded' ) ),
			),
		),
	)
);
check( 'guarded-only item: 0 counted findings, still compatible', 0 === $guarded_only['targets']['php:8.4']['findings_total'] && 'compatible' === leapcheck_item_verdict( array( 'status' => 'ok', 'data' => $guarded_only ), '8.4' ) );

// ---- Requirements --------------------------------------------------------------------
$req_rows = array(
	row( 'a', true, array(), array( array( 'name' => 'intl' ), array( 'name' => 'gd' ) ) ),
	row( 'b', true, array(), array( array( 'name' => 'gd' ), array( 'name' => 'mbstring' ) ) ),
	row( 'c', false, array(), array( array( 'name' => 'soap' ) ) ),
	row( 'd', true, array(), array( array( 'name' => 'zip' ) ), '0.9' ),
	array( 'item' => $custom, 'record' => array( 'status' => 'missing' ) ),
);
check( 'requirements: union of items in use, sorted, inactive ignored', array( 'gd', 'intl', 'mbstring', 'zip' ) === leapcheck_collect_requirements( $req_rows ) );
check( 'requirements: none', array() === leapcheck_collect_requirements( array( $rows[1] ) ) );

// ---- Library conflicts -------------------------------------------------------------------
$conf = leapcheck_find_conflicts(
	array(
		row( 'forms', true, array( lib( 'PHPMailer', '6.9.1' ), lib( 'guzzle', '7.8.0' ) ) ),
		row( 'newsletter', true, array( lib( 'phpmailer', '5.2.7' ), lib( 'guzzle', '7.8.0' ) ) ),
		row( 'shop', true, array( lib( 'phpmailer', '6.9.1' ) ) ),
	)
);
check( 'conflict: one library flagged (same-version guzzle is fine)', 1 === count( $conf ) && 'PHPMailer' === $conf[0]['library'] );
check( 'conflict: copies listed oldest first', '5.2.7' === $conf[0]['copies'][0]['version'] && 'Newsletter' === $conf[0]['copies'][0]['item'] && 3 === count( $conf[0]['copies'] ) );
check(
	'conflict: inactive plugins ignored',
	array() === leapcheck_find_conflicts( array( row( 'a', true, array( lib( 'x', '1.0' ) ) ), row( 'b', false, array( lib( 'x', '2.0' ) ) ) ) )
);
check(
	'conflict: closest-version results ignored',
	array() === leapcheck_find_conflicts( array( row( 'a', true, array( lib( 'x', '1.0' ) ) ), row( 'b', true, array( lib( 'x', '2.0' ) ), array(), '0.9' ) ) )
);
check(
	'conflict: JavaScript libraries ignored',
	array() === leapcheck_find_conflicts( array( row( 'a', true, array( lib( 'jquery', '1.12.4', 'js' ) ) ), row( 'b', true, array( lib( 'jquery', '3.7.1', 'js' ) ) ) ) )
);
check(
	'conflict: unknown versions ignored',
	array() === leapcheck_find_conflicts( array( row( 'a', true, array( lib( 'x', '' ) ) ), row( 'b', true, array( lib( 'x', '2.0' ) ) ) ) )
);
check(
	'conflict: two versions inside one plugin only is not a cross-plugin conflict',
	array() === leapcheck_find_conflicts( array( row( 'a', true, array( lib( 'x', '1.0' ), lib( 'x', '2.0' ) ) ), row( 'b', true, array() ) ) )
);
check(
	'conflict: items that did not load ignored',
	array() === leapcheck_find_conflicts( array( row( 'a', true, array( lib( 'x', '1.0' ) ) ), array( 'item' => $custom, 'record' => array( 'status' => 'missing' ) ) ) )
);

echo $failures ? "\n$failures failure(s)\n" : "\nAll tests passed\n";
exit( $failures ? 1 : 0 );
