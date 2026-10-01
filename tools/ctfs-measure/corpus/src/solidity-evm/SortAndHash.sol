// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

contract SortAndHash {
    mapping(uint256 => uint256) public balances;
    uint256[] public data;

    function fib(uint256 n) internal pure returns (uint256) {
        uint256 a = 0;
        uint256 b = 1;
        for (uint256 i = 0; i < n; i++) {
            uint256 t = a + b;
            a = b;
            b = t;
        }
        return a;
    }

    function sortArr(uint256[] memory arr) internal pure returns (uint256[] memory) {
        uint256 n = arr.length;
        for (uint256 i = 0; i < n; i++) {
            for (uint256 j = 0; j + 1 < n - i; j++) {
                if (arr[j] > arr[j + 1]) {
                    uint256 tmp = arr[j];
                    arr[j] = arr[j + 1];
                    arr[j + 1] = tmp;
                }
            }
        }
        return arr;
    }

    function run() public returns (uint256) {
        uint256[] memory arr = new uint256[](40);
        uint256 seed = 12345;
        for (uint256 i = 0; i < arr.length; i++) {
            seed = uint256(keccak256(abi.encodePacked(seed, i))) % 1000;
            arr[i] = seed;
        }
        arr = sortArr(arr);
        uint256 acc = 0;
        for (uint256 i = 0; i < arr.length; i++) {
            acc += arr[i] * fib(i % 20);
            if (i % 5 == 0) {
                balances[i] = acc;
                data.push(acc);
            }
        }
        return acc;
    }
}
