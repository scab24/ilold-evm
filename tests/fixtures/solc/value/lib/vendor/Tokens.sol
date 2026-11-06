// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

interface IERC20 {
    function transfer(address to, uint256 amount) external returns (bool);

    function transferFrom(address from, address to, uint256 amount) external returns (bool);

    function allowance(address owner, address spender) external view returns (uint256);

    function balanceOf(address owner) external view returns (uint256);
}

interface IMintableToken is IERC20 {
    function mint(address to, uint256 amount) external;

    function burn(uint256 amount) external;

    function burn(address from, uint256 amount) external;

    function burnFrom(address from, uint256 amount) external;
}

interface IERC721 {
    function balanceOf(address owner) external view returns (uint256);

    function ownerOf(uint256 id) external view returns (address);

    function transferFrom(address from, address to, uint256 id) external;

    function safeTransferFrom(address from, address to, uint256 id) external;

    function safeTransferFrom(address from, address to, uint256 id, bytes calldata data) external;
}

interface IMintableNft is IERC721 {
    function mint(address to, uint256 id) external;

    function burn(uint256 id) external;
}

interface IERC1155 {
    function balanceOf(address owner, uint256 id) external view returns (uint256);

    function safeTransferFrom(address from, address to, uint256 id, uint256 amount, bytes calldata data) external;

    function safeBatchTransferFrom(
        address from,
        address to,
        uint256[] calldata ids,
        uint256[] calldata amounts,
        bytes calldata data
    ) external;
}

interface IMintableMulti is IERC1155 {
    function mint(address to, uint256 id, uint256 amount, bytes calldata data) external;
}

interface IERC6909 {
    function balanceOf(address owner, uint256 id) external view returns (uint256);

    function transfer(address to, uint256 id, uint256 amount) external returns (bool);

    function transferFrom(address from, address to, uint256 id, uint256 amount) external returns (bool);
}

/// Burns without saying which standard it follows.
interface IBurnable {
    function burn(uint256 amount) external;
}
