<?php
/**
 * Site-independent test of the pure logic in includes/functions.php.
 * Run: php plugin/tests/test-functions.php   (exit code 0 = pass)
 *
 * @package LeapCheck
 */

define( 'ABSPATH', __DIR__ . '/' );
define( 'LEAPCHECK_VERSION', '0.1.0' );
define( 'LEAPCHECK_BASE_URL', 'https://leapcheck.vercel.app' );

// Minimal stubs for the few WordPress functions the pure helpers call.
function apply_filters( $hook, $value ) {
	return $value;
}
function esc_url_raw( $url ) {
	return filter_var( $url, FILTER_VALIDATE_URL ) ? $url : '';
}
function untrailingslashit( $value ) {
	return rtrim( $value, '/\\' );
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

// Parsing the docs/07 fixture.
$raw    = json_decode( file_get_contents( __DIR__ . '/sample-result.json' ), true );
$parsed = leapcheck_parse_result( $raw );
check( 'fixture parses', is_array( $parsed ) );
check( 'three targets', 3 === count( $parsed['targets'] ) );
check( 'php:8.4 has 2 findings', 2 === count( $parsed['targets']['php:8.4']['findings'] ) );
check( 'finding line is int', 161 === $parsed['targets']['php:8.4']['findings'][0]['line'] );
check( 'null component becomes empty string', '' === $parsed['targets']['php:8.4']['findings'][0]['component'] );
check( 'component parsed', 'phpmailer' === $parsed['components'][0]['name'] && '5.2.7' === $parsed['components'][0]['version'] );

// Invalid input.
check( 'rejects wrong schema', null === leapcheck_parse_result( array( 'schema' => 2, 'targets' => array() ) ) );
check( 'rejects non-array', null === leapcheck_parse_result( 'nope' ) );
$odd = leapcheck_parse_result( array( 'schema' => 1, 'targets' => array( 'php:8.4' => array( 'verdict' => '<script>' ) ) ) );
check( 'unknown verdict string becomes unknown', 'unknown' === $odd['targets']['php:8.4']['verdict'] );

// Verdicts.
$ok = array( 'status' => 'ok', 'data' => $parsed );
check( '8.3 compatible', 'compatible' === leapcheck_item_verdict( $ok, '8.3' ) );
check( '8.4 issues', 'issues' === leapcheck_item_verdict( $ok, '8.4' ) );
check( '8.5 breaks', 'breaks' === leapcheck_item_verdict( $ok, '8.5' ) );
check( 'missing target is unknown', 'unknown' === leapcheck_item_verdict( $ok, '9.0' ) );
check( '404 is missing', 'missing' === leapcheck_item_verdict( array( 'status' => 'missing' ), '8.4' ) );
check( 'fetch error is error', 'error' === leapcheck_item_verdict( array( 'status' => 'error' ), '8.4' ) );

// Items and URLs.
$item = leapcheck_make_item( 'plugin', 'contact-form-7', 'Contact Form 7', '5.1', true );
check( 'plugin url', 'https://leapcheck.vercel.app/v1/wordpress-plugin/contact-form-7/5.1.json' === leapcheck_result_url( $item ) );
$theme = leapcheck_make_item( 'theme', 'TwentyTwentyFour', 'Twenty Twenty-Four', '1.2', true, 'active-theme' );
check( 'theme url, slug lowercased', 'https://leapcheck.vercel.app/v1/wordpress-theme/twentytwentyfour/1.2.json' === leapcheck_result_url( $theme ) );
check( 'path traversal rejected', false === leapcheck_make_item( 'plugin', '../x', 'X', '1.0', true )['valid'] );
check( 'empty version rejected', false === leapcheck_make_item( 'plugin', 'x', 'X', '', true )['valid'] );

// Summary.
$rows = array(
	array( 'item' => $item, 'record' => $ok ),
	array( 'item' => leapcheck_make_item( 'plugin', 'custom', 'Custom', '1.0', true ), 'record' => array( 'status' => 'missing' ) ),
	array( 'item' => leapcheck_make_item( 'plugin', 'old', 'Old', '1.0', false ), 'record' => array( 'status' => 'missing' ) ),
);
$s83 = leapcheck_summarize( $rows, '8.3' );
check( 'summary 8.3 unchecked (one missing)', 'unchecked' === $s83['state'] && 2 === $s83['total'] && 1 === $s83['unchecked'] );
check( 'summary 8.4 issues', 'issues' === leapcheck_summarize( $rows, '8.4' )['state'] );
check( 'summary 8.5 breaks', 'breaks' === leapcheck_summarize( $rows, '8.5' )['state'] );
check( 'summary all compatible', 'compatible' === leapcheck_summarize( array( $rows[0] ), '8.3' )['state'] );
check( 'summary empty', 'empty' === leapcheck_summarize( array( $rows[2] ), '8.3' )['state'] );

echo $failures ? "\n$failures failure(s)\n" : "\nAll tests passed\n";
exit( $failures ? 1 : 0 );
